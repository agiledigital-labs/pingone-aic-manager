//! Shared parsing for command-local `--force[=<guard>]` permissions.
//!
//! Commands opt into exactly the guards they support by flattening one of the
//! aliases in this module into their clap-derived arguments:
//!
//! ```text
//! #[command(flatten)]
//! force: OperationAndBackupForce,
//! ```
//!
//! Add an alias when a later command needs another combination. Keeping the
//! allowed set in the field's type makes clap reject unsupported guard names
//! during parsing, before dispatch or dry-run handling. Callers then ask the
//! parsed value about one permission at a time (`operation()`,
//! `syntax_check()`, or `backup()`).

use clap::builder::{PossibleValuesParser, TypedValueParser};
use clap::{Arg, ArgAction, ArgMatches, Args, Command, Error, FromArgMatches};

const OPERATION: u8 = 1 << 0;
const SYNTAX_CHECK: u8 = 1 << 1;
const BACKUP: u8 = 1 << 2;
const KNOWN_GUARDS: u8 = OPERATION | SYNTAX_CHECK | BACKUP;

/// Parsed `--force` permissions for one command invocation.
///
/// `ALLOWED` is deliberately hidden behind the named aliases below. It is a
/// clap-construction detail, while the methods are the command logic API.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ForceFlags<const ALLOWED: u8> {
    requested: u8,
}

/// A command whose bare `--force` authorizes its primary safety override.
pub type OperationForce = ForceFlags<OPERATION>;

/// A command supporting only `--force=syntax-check`.
pub type SyntaxCheckForce = ForceFlags<SYNTAX_CHECK>;

/// A command supporting its primary override and syntax-check bypass.
pub type OperationAndSyntaxCheckForce = ForceFlags<{ OPERATION | SYNTAX_CHECK }>;

/// A command supporting its primary override and backup bypass.
pub type OperationAndBackupForce = ForceFlags<{ OPERATION | BACKUP }>;

impl<const ALLOWED: u8> ForceFlags<ALLOWED> {
    /// Whether bare `--force` or explicit `--force=operation` was supplied.
    pub fn operation(&self) -> bool {
        self.requested & OPERATION != 0
    }

    /// Whether `--force=syntax-check` was supplied.
    pub fn syntax_check(&self) -> bool {
        self.requested & SYNTAX_CHECK != 0
    }

    /// Whether `--force=backup` was supplied.
    pub fn backup(&self) -> bool {
        self.requested & BACKUP != 0
    }

    fn insert(&mut self, guard: ForceGuard) {
        self.requested |= guard.bit();
    }
}

impl<const ALLOWED: u8> FromArgMatches for ForceFlags<ALLOWED> {
    fn from_arg_matches(matches: &ArgMatches) -> Result<Self, Error> {
        let mut matches = matches.clone();
        Self::from_arg_matches_mut(&mut matches)
    }

    fn from_arg_matches_mut(matches: &mut ArgMatches) -> Result<Self, Error> {
        let mut flags = Self::default();
        if let Some(guards) = matches.remove_many::<ForceGuard>("force") {
            for guard in guards {
                flags.insert(guard);
            }
        }
        Ok(flags)
    }

    fn update_from_arg_matches(&mut self, matches: &ArgMatches) -> Result<(), Error> {
        let mut matches = matches.clone();
        self.update_from_arg_matches_mut(&mut matches)
    }

    fn update_from_arg_matches_mut(&mut self, matches: &mut ArgMatches) -> Result<(), Error> {
        if let Some(guards) = matches.remove_many::<ForceGuard>("force") {
            for guard in guards {
                self.insert(guard);
            }
        }
        Ok(())
    }
}

impl<const ALLOWED: u8> Args for ForceFlags<ALLOWED> {
    fn augment_args(command: Command) -> Command {
        command.arg(force_arg::<ALLOWED>())
    }

    fn augment_args_for_update(command: Command) -> Command {
        command.arg(force_arg::<ALLOWED>())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ForceGuard {
    Operation,
    SyntaxCheck,
    Backup,
}

impl ForceGuard {
    fn bit(self) -> u8 {
        match self {
            Self::Operation => OPERATION,
            Self::SyntaxCheck => SYNTAX_CHECK,
            Self::Backup => BACKUP,
        }
    }

    fn from_name(name: String) -> Self {
        match name.as_str() {
            "operation" => Self::Operation,
            "syntax-check" => Self::SyntaxCheck,
            "backup" => Self::Backup,
            _ => unreachable!("PossibleValuesParser returned an unknown force guard"),
        }
    }
}

fn force_arg<const ALLOWED: u8>() -> Arg {
    assert_eq!(
        ALLOWED & !KNOWN_GUARDS,
        0,
        "ForceFlags contains an unknown allowed-guard bit"
    );

    Arg::new("force")
        .long("force")
        .value_name("GUARD")
        .help("Authorize the primary safety override, or only a named guard")
        .num_args(0..=1)
        .require_equals(true)
        .default_missing_value("operation")
        .action(ArgAction::Append)
        .value_parser(force_value_parser::<ALLOWED>())
}

fn force_value_parser<const ALLOWED: u8>() -> impl TypedValueParser<Value = ForceGuard> {
    let mut names = Vec::new();
    if ALLOWED & OPERATION != 0 {
        names.push("operation");
    }
    if ALLOWED & SYNTAX_CHECK != 0 {
        names.push("syntax-check");
    }
    if ALLOWED & BACKUP != 0 {
        names.push("backup");
    }

    PossibleValuesParser::new(names).map(ForceGuard::from_name)
}

#[cfg(test)]
mod tests {
    use clap::{Parser, Subcommand};

    use super::*;

    type EveryForce = ForceFlags<KNOWN_GUARDS>;

    #[derive(Debug, Parser)]
    struct Cli {
        #[command(subcommand)]
        command: TestCommand,
    }

    #[derive(Debug, Subcommand)]
    enum TestCommand {
        Run {
            /// A positional proves that optional force values require `=`.
            name: Option<String>,

            #[arg(long)]
            dry_run: bool,

            #[command(flatten)]
            force: EveryForce,
        },
    }

    #[derive(Debug, Parser)]
    struct SyntaxOnlyCli {
        #[command(flatten)]
        force: SyntaxCheckForce,
    }

    fn parse(args: &[&str]) -> (Option<String>, bool, EveryForce) {
        let cli =
            Cli::try_parse_from(["test", "run"].into_iter().chain(args.iter().copied())).unwrap();
        match cli.command {
            TestCommand::Run {
                name,
                dry_run,
                force,
            } => (name, dry_run, force),
        }
    }

    #[test]
    fn bare_force_requests_only_the_operation_guard() {
        let (_, _, force) = parse(&["--force"]);

        assert!(force.operation());
        assert!(!force.syntax_check());
        assert!(!force.backup());
    }

    #[test]
    fn named_force_requests_only_that_guard() {
        let (_, _, force) = parse(&["--force=syntax-check"]);

        assert!(!force.operation());
        assert!(force.syntax_check());
        assert!(!force.backup());
    }

    #[test]
    fn bare_and_named_force_accumulate_independent_permissions() {
        let (_, _, force) = parse(&["--force", "--force=syntax-check"]);

        assert!(force.operation());
        assert!(force.syntax_check());
        assert!(!force.backup());
    }

    #[test]
    fn space_after_force_belongs_to_the_positional() {
        let (name, _, force) = parse(&["--force", "syntax-check"]);

        assert_eq!(name.as_deref(), Some("syntax-check"));
        assert!(force.operation());
        assert!(!force.syntax_check());
    }

    #[test]
    fn an_unknown_guard_is_a_parse_error() {
        let error =
            Cli::try_parse_from(["test", "run", "--dry-run", "--force=unknown"]).unwrap_err();

        assert_eq!(error.kind(), clap::error::ErrorKind::InvalidValue);
    }

    #[test]
    fn duplicate_named_guards_are_harmless() {
        let (_, _, force) = parse(&["--force=backup", "--force=backup"]);

        assert!(force.backup());
        assert!(!force.operation());
        assert!(!force.syntax_check());
    }

    #[test]
    fn explicit_operation_has_the_same_narrow_meaning_as_bare_force() {
        let (_, _, force) = parse(&["--force=operation"]);

        assert!(force.operation());
        assert!(!force.syntax_check());
        assert!(!force.backup());
    }

    #[test]
    fn a_commands_allowed_set_is_enforced_during_parsing() {
        assert!(SyntaxOnlyCli::try_parse_from(["test", "--force=syntax-check"]).is_ok());
        assert!(SyntaxOnlyCli::try_parse_from(["test", "--force=backup"]).is_err());
        assert!(SyntaxOnlyCli::try_parse_from(["test", "--force"]).is_err());
    }

    #[test]
    fn all_is_not_a_wildcard() {
        assert!(Cli::try_parse_from(["test", "run", "--force=all"]).is_err());
    }
}
