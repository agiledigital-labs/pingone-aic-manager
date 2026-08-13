//! Single source of truth for key bindings.
//!
//! Dispatch, footer hints, and the F1 help popover all derive from the same
//! per-mode binding tables, so they can't drift — the class of bug where the
//! footer advertised `^S` but the handler ignored it in the secrets view.
//!
//! Rolled out per mode. Normal mode (the dashboard, where the drift bit) is
//! table-driven here; other modes are being migrated incrementally.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, InputMode, Realm, View};
use crate::esv::screen::Mode as EsvMode;
use crate::esv::state::EsvView;
use crate::onboard::screen::Mode as OnboardMode;

/// One key that fires a binding. Matching is by code + the ctrl modifier.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    Char(char),
    Ctrl(char),
    Code(KeyCode),
}

impl Trigger {
    // Named constants for the keys that appear in nearly every form table.
    // `Trigger::Code(KeyCode::Tab)` spelled out at every call site turns a
    // binding table into something you can't read as a table.
    pub const TAB: Self = Trigger::Code(KeyCode::Tab);
    pub const BACKTAB: Self = Trigger::Code(KeyCode::BackTab);
    pub const ENTER: Self = Trigger::Code(KeyCode::Enter);
    pub const ESC: Self = Trigger::Code(KeyCode::Esc);
    pub const LEFT: Self = Trigger::Code(KeyCode::Left);
    pub const RIGHT: Self = Trigger::Code(KeyCode::Right);
    pub const UP: Self = Trigger::Code(KeyCode::Up);
    pub const DOWN: Self = Trigger::Code(KeyCode::Down);
    pub const SPACE: Self = Trigger::Char(' ');

    pub fn matches(self, key: &KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match self {
            Trigger::Char(c) => !ctrl && key.code == KeyCode::Char(c),
            Trigger::Ctrl(c) => ctrl && key.code == KeyCode::Char(c),
            Trigger::Code(code) => key.code == code,
        }
    }
}

/// What a Normal-mode binding does. Kept as a plain enum so dispatch is one
/// `match` and the same binding list feeds the hint renderers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Act {
    Quit,
    Functions,
    NextView,
    PrevView,
    Search,
    ClearFilter,
    MoveDown,
    MoveUp,
    Top,
    Bottom,
    PageDown,
    PageUp,
    Primary,
    Delete,
    NewItem,
    RenameField,
    RenameObject,
    DeleteObject,
    NewObject,
    AddHook,
    Pull,
    Push,
    PullAll,
    ReconMapping,
    PullMappingScripts,
    Apply,
    Refresh,
    Undo,
    UndoHistory,
    PrevField,
    NextField,
    DetailScrollDown,
    DetailScrollUp,
    RealmToggle,
    TenantPicker,
    Onboard,
    AuthSettings,
    Lock,
    /// Reopen the idle-lock prompt after the user dismissed it.
    Relock,
}

/// A single binding: which keys fire it, how it's labelled in the footer / F1
/// help, and the action it runs.
///
/// Generic over the action so each feature can define its own. A mode's binding
/// table is the single description of what its keys do: dispatch, the footer,
/// and the F1 overlay all derive from it, which is what stops the three from
/// drifting apart.
pub struct Bind<A = Act> {
    pub triggers: &'static [Trigger],
    pub label: &'static str,
    pub desc: &'static str,
    pub footer: bool,
    pub help: bool,
    pub act: A,
}

pub const fn b<A>(
    triggers: &'static [Trigger],
    label: &'static str,
    desc: &'static str,
    footer: bool,
    help: bool,
    act: A,
) -> Bind<A> {
    Bind {
        triggers,
        label,
        desc,
        footer,
        help,
        act,
    }
}

/// A binding shown in both the footer and the F1 overlay — the common case.
pub const fn hint<A>(
    triggers: &'static [Trigger],
    label: &'static str,
    desc: &'static str,
    act: A,
) -> Bind<A> {
    b(triggers, label, desc, true, true, act)
}

/// A binding listed only in the F1 overlay. For keys worth documenting but not
/// worth footer width — movement, conventions the user already knows.
pub const fn help_only<A>(
    triggers: &'static [Trigger],
    label: &'static str,
    desc: &'static str,
    act: A,
) -> Bind<A> {
    b(triggers, label, desc, false, true, act)
}

/// A binding that fires but is never advertised. Label and description are kept
/// anyway: they document the intent, and they're what you'd want if the key is
/// ever promoted to a hint.
pub const fn hidden<A>(
    triggers: &'static [Trigger],
    label: &'static str,
    desc: &'static str,
    act: A,
) -> Bind<A> {
    b(triggers, label, desc, false, false, act)
}

/// `^S` for a form row where `Enter` won't commit. Advertised, because here it
/// is the only way to save without tabbing to the Save button — the inverse
/// case shares one binding with `Enter` and is labelled `Enter` instead.
///
/// `verb` is the form's own word for committing ("save" / "add" / "create"), so
/// the footer says what the key does.
pub const fn save_chord_bind<A>(act: A, verb: &'static str) -> Bind<A> {
    hint(&[Trigger::Ctrl('s')], "^S", verb, act)
}

impl<A: Copy> Bind<A> {
    /// The action bound to `key` in `binds`, if any. Table order decides
    /// precedence, so the first match wins.
    pub fn resolve(binds: &[Self], key: &KeyEvent) -> Option<A> {
        binds
            .iter()
            .find(|bind| bind.triggers.iter().any(|t| t.matches(key)))
            .map(|bind| bind.act)
    }

    /// Footer hints for `binds`, in table order.
    pub fn footer_hints(binds: &[Self]) -> Vec<(&'static str, &'static str)> {
        Self::hints(binds, |bind| bind.footer)
    }

    /// F1 overlay rows for `binds`, in table order.
    pub fn help_hints(binds: &[Self]) -> Vec<(&'static str, &'static str)> {
        Self::hints(binds, |bind| bind.help)
    }

    fn hints(binds: &[Self], include: impl Fn(&Self) -> bool) -> Vec<(&'static str, &'static str)> {
        binds
            .iter()
            .filter(|bind| include(bind))
            .map(|bind| (bind.label, bind.desc))
            .collect()
    }
}

/// Which binding subset a renderer needs.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HintTarget {
    Footer,
    Help,
}

/// Select footer or help rows from a binding table, preserving table order.
pub fn pick<A: Copy>(binds: &[Bind<A>], target: HintTarget) -> Vec<(&'static str, &'static str)> {
    match target {
        HintTarget::Footer => Bind::footer_hints(binds),
        HintTarget::Help => Bind::help_hints(binds),
    }
}

/// The Normal-mode bindings for the current state. Conditional on active view,
/// and selection so the footer never advertises a key that won't fire.
pub fn normal_binds(app: &App) -> Vec<Bind> {
    use Act::*;
    let mut out: Vec<Bind> = Vec::new();

    // Help-only: the header already shows `Ctrl-P`, so repeating it in the
    // footer buys nothing but width.
    out.push(help_only(
        &[Trigger::Ctrl('p')],
        "Ctrl-P",
        "functions",
        Functions,
    ));

    // First-run / no tenants: only the bootstrap shortcuts make sense.
    if app.tenants.is_empty() {
        out.push(hint(&[Trigger::Ctrl('t')], "^T", "add tenant", Onboard));
        out.push(hint(
            &[Trigger::Ctrl('a')],
            "^A",
            "auth settings",
            AuthSettings,
        ));
        push_global(&mut out);
        return out;
    }

    let scripts_view = app.active_view == View::Scripts;
    let esv_view_active = app.active_view == View::Esvs;
    let managed_view = app.active_view == View::Managed;
    let mappings_view = app.active_view == View::Mappings;
    let access_view = app.active_view == View::Access;
    let idmstore_view = app.active_view == View::IdmStore;
    let oauth_view = app.active_view == View::Oauth;
    let mappings_allowed = mappings_allowed(app);
    let esv_view = app.esv.view.clamp(mappings_allowed);
    let secrets = esv_view_active && esv_view == EsvView::Secrets;
    let mappings = esv_view_active && esv_view == EsvView::Mappings;
    let n = row_count(app);
    let can_apply = esv_view_active
        && app
            .active_tenant()
            .map(|t| crate::esv::state::can_request_restart(app, &t.name))
            .unwrap_or(false);

    if can_apply {
        out.push(hint(&[Trigger::Ctrl('s')], "^S", "apply changes", Apply));
    }
    // `[` / `]` switch the ESV view's inner sections; mappings are only present
    // on lower-environment tenants.
    if esv_view_active {
        let next_label = match (esv_view, mappings_allowed) {
            (EsvView::Variables, _) => "secrets",
            (EsvView::Secrets, true) => "mappings",
            (EsvView::Secrets, false) => "variables",
            (EsvView::Mappings, _) => "variables",
        };
        let prev_label = match (esv_view, mappings_allowed) {
            (EsvView::Variables, true) => "mappings",
            (EsvView::Variables, false) => "secrets",
            (EsvView::Secrets, _) => "variables",
            (EsvView::Mappings, _) => "secrets",
        };
        out.push(hint(&[Trigger::Char('[')], "[", prev_label, PrevView));
        out.push(hint(&[Trigger::Char(']')], "]", next_label, NextView));
    }
    // Help-only, like the movement keys below: `/` for search is a convention
    // the footer doesn't need to spend width teaching.
    out.push(help_only(&[Trigger::Char('/')], "/", "search", Search));

    // Movement (help-only; the footer stays uncluttered).
    out.push(help_only(
        &[Trigger::Char('j'), Trigger::DOWN],
        "↓",
        "move down",
        MoveDown,
    ));
    out.push(help_only(
        &[Trigger::Char('k'), Trigger::UP],
        "↑",
        "move up",
        MoveUp,
    ));
    out.push(help_only(&[Trigger::Char('g')], "g", "top", Top));
    out.push(help_only(&[Trigger::Char('G')], "G", "bottom", Bottom));
    out.push(help_only(
        &[Trigger::Code(KeyCode::PageDown)],
        "PgDn",
        "page down",
        PageDown,
    ));
    out.push(help_only(
        &[Trigger::Code(KeyCode::PageUp)],
        "PgUp",
        "page up",
        PageUp,
    ));

    if scripts_view {
        if n > 0 {
            out.push(hint(
                &[Trigger::Char('p'), Trigger::ENTER],
                "p",
                "pull",
                Pull,
            ));
            out.push(hint(&[Trigger::Char('P')], "P", "push", Push));
        }
        out.push(hint(&[Trigger::Char('a')], "a", "pull all", PullAll));
    } else if esv_view_active {
        if n > 0 {
            if secrets {
                out.push(hint(
                    &[Trigger::ENTER, Trigger::Char('v')],
                    "Enter",
                    "versions",
                    Primary,
                ));
            } else if mappings {
                out.push(hint(&[Trigger::Char('e')], "e", "edit alias", Primary));
                out.push(hint(
                    &[Trigger::Char('d'), Trigger::Char('D')],
                    "d",
                    "remove",
                    Delete,
                ));
            } else {
                out.push(hint(&[Trigger::ENTER], "Enter", "edit", Primary));
            }
            if !mappings {
                out.push(hint(
                    &[Trigger::Char('d'), Trigger::Char('D')],
                    "d",
                    "delete",
                    Delete,
                ));
            }
        }

        if !mappings {
            out.push(hint(
                &[Trigger::Ctrl('n')],
                "^N",
                if secrets {
                    "new secret"
                } else {
                    "new variable"
                },
                NewItem,
            ));
        } else {
            out.push(hint(&[Trigger::Char('a')], "a", "add", NewItem));
        }
        if mappings && n > 0 {
            // Help-only, unlike Access and OAuth, which advertise these in the
            // footer because their detail pane *is* the content. Here the pane
            // is a strip beside a list, the ESVs footer is the busiest in the
            // app, and pane scrolling is movement — which DESIGN.md puts in the
            // popover rather than every footer.
            out.push(help_only(
                &[Trigger::Ctrl('d')],
                "^D",
                "scroll detail down",
                DetailScrollDown,
            ));
            out.push(help_only(
                &[Trigger::Ctrl('u')],
                "^U",
                "scroll detail up",
                DetailScrollUp,
            ));
        }
        out.push(hint(&[Trigger::Ctrl('z')], "^Z", "undo", Undo));
        out.push(hint(
            &[Trigger::Ctrl('y')],
            "^Y",
            "undo history",
            UndoHistory,
        ));
    } else if managed_view && n > 0 {
        out.push(hint(&[Trigger::ENTER], "Enter", "edit field", Primary));
        out.push(hint(&[Trigger::Char('a')], "a", "add", NewItem));
        out.push(hint(
            &[Trigger::Char('r')],
            "r",
            "rename field",
            RenameField,
        ));
        out.push(hint(
            &[Trigger::Char('R')],
            "R",
            "rename object",
            RenameObject,
        ));
        out.push(hint(&[Trigger::Char('h')], "h", "add hook", AddHook));
        out.push(hint(&[Trigger::Char('d')], "d", "delete field", Delete));
        out.push(hint(
            &[Trigger::Char('D')],
            "D",
            "delete object",
            DeleteObject,
        ));
        out.push(help_only(
            &[Trigger::Char('[')],
            "[",
            "previous field",
            PrevField,
        ));
        out.push(help_only(
            &[Trigger::Char(']')],
            "]",
            "next field",
            NextField,
        ));
        out.push(hint(&[Trigger::Ctrl('z')], "^Z", "undo", Undo));
        out.push(hint(
            &[Trigger::Ctrl('y')],
            "^Y",
            "undo history",
            UndoHistory,
        ));
    } else if mappings_view && n > 0 {
        out.push(hint(&[Trigger::Char('r')], "r", "reconcile", ReconMapping));
        out.push(hint(
            &[Trigger::Char('p')],
            "p",
            "pull scripts",
            PullMappingScripts,
        ));
    } else if access_view {
        if n > 0 {
            out.push(hint(&[Trigger::ENTER], "Enter", "edit", Primary));
            out.push(hint(
                &[
                    Trigger::Char('d'),
                    Trigger::Char('D'),
                    Trigger::Code(KeyCode::Delete),
                ],
                "d",
                "delete",
                Delete,
            ));
            out.push(hint(
                &[Trigger::Ctrl('d')],
                "^D",
                "scroll detail down",
                DetailScrollDown,
            ));
            out.push(hint(
                &[Trigger::Ctrl('u')],
                "^U",
                "scroll detail up",
                DetailScrollUp,
            ));
        }
        out.push(hint(&[Trigger::Ctrl('n')], "^N", "new rule", NewItem));
        out.push(hint(&[Trigger::Ctrl('z')], "^Z", "undo", Undo));
        out.push(hint(
            &[Trigger::Ctrl('y')],
            "^Y",
            "undo history",
            UndoHistory,
        ));
    } else if oauth_view && n > 0 {
        out.push(hint(&[Trigger::ENTER], "Enter", "inspect", Primary));
        out.push(hint(
            &[Trigger::Ctrl('d')],
            "^D",
            "scroll detail down",
            DetailScrollDown,
        ));
        out.push(hint(
            &[Trigger::Ctrl('u')],
            "^U",
            "scroll detail up",
            DetailScrollUp,
        ));
    }
    if managed_view {
        out.push(hint(&[Trigger::Ctrl('n')], "^N", "new object", NewObject));
    }
    if managed_view || mappings_view || access_view || idmstore_view || oauth_view || mappings {
        out.push(hint(&[Trigger::Ctrl('r')], "^R", "refresh", Refresh));
    }

    // Esc clears an active filter (only meaningful when one is applied).
    if filter_active(app) {
        out.push(help_only(
            &[Trigger::ESC],
            "Esc",
            "clear filter",
            ClearFilter,
        ));
    }

    // Global commands available on every populated screen. Realm toggle only
    // applies to the ESV view — scripts are addressed by namespace, not realm.
    if esv_view_active {
        let realm_triggers: &'static [Trigger] = if mappings {
            &[Trigger::Char('r')]
        } else {
            &[Trigger::Char('r'), Trigger::Char('R')]
        };
        out.push(help_only(
            realm_triggers,
            if mappings { "r" } else { "r/R" },
            "switch realm",
            RealmToggle,
        ));
    }
    out.push(help_only(
        &[Trigger::Char('t'), Trigger::Char('T')],
        "t",
        "switch tenant",
        TenantPicker,
    ));
    out.push(help_only(
        &[Trigger::Ctrl('t')],
        "^T",
        "add tenant",
        Onboard,
    ));
    out.push(help_only(
        &[Trigger::Ctrl('a')],
        "^A",
        "auth settings",
        AuthSettings,
    ));
    out.push(help_only(&[Trigger::Char('L')], "L", "lock & quit", Lock));
    // Only reachable — and only worth footer width — while the user has an
    // outstanding dismissed relock prompt. Without it, dismissing the prompt
    // would strand them with a locked agent and no way to re-authenticate.
    if app.unlock.relock_dismissed {
        out.push(hint(&[Trigger::Ctrl('l')], "^L", "unlock session", Relock));
    }
    push_global(&mut out);
    out
}

/// The footer hint bar's contents for the current mode — the single source for
/// `header::draw_hints`. Normal comes from the binding table (footer subset);
/// the two dashboard text modes have small curated sets; every other mode is a
/// full-screen modal that renders its own chrome hints, so the bar is empty.
pub fn footer_hints(app: &App) -> Vec<(&'static str, &'static str)> {
    match app.input_mode {
        InputMode::Normal => {
            let mut out: Vec<(&str, &str)> = normal_binds(app)
                .iter()
                .filter(|bind| bind.footer)
                .map(|bind| (bind.label, bind.desc))
                .collect();
            out.push(("?", "keys"));
            out
        }
        InputMode::Scripts(_) => {
            vec![("Enter", "keep filter"), ("Esc", "clear + exit")]
        }
        InputMode::Esv(_) => crate::esv::screen::footer_hints(app),
        InputMode::Managed(_) => crate::managed::screen::footer_hints(app),
        InputMode::Mappings(_) => crate::mappings::screen::footer_hints(app),
        InputMode::Access(_) => crate::access::screen::footer_hints(app),
        InputMode::IdmStore(_) => crate::idmstore::screen::footer_hints(app),
        InputMode::Oauth(_) => crate::oauth::screen::footer_hints(app),
        InputMode::Selector => Vec::new(),
        InputMode::Secretmap(_) => crate::secretmap::screen::footer_hints(app),
        InputMode::Secrets(_) => crate::secrets::screen::footer_hints(app),
        InputMode::Vault(_) => Vec::new(),
        _ => Vec::new(),
    }
}

/// Quit bindings — present in every Normal state, never shown as hints.
fn push_global(out: &mut Vec<Bind>) {
    out.push(hidden(&[Trigger::Char('q')], "q", "quit", Act::Quit));
    out.push(hidden(&[Trigger::Ctrl('c')], "^C", "quit", Act::Quit));
}

/// The single key-dispatch entry point for every input mode. `app::handle_key`
/// calls this after the keybind-help-popover pre-checks. Normal mode is fully
/// table-driven (below); other modes route to their screen handler — one place
/// that maps mode → handler, instead of a match scattered in `app`.
pub async fn dispatch(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    match app.input_mode {
        InputMode::Normal => dispatch_normal(app, key).await?,
        InputMode::Vault(mode) => crate::vault::screen::handle_key(app, key, mode).await?,
        InputMode::Onboard(mode) => crate::onboard::screen::handle_key(app, key, mode).await?,
        InputMode::EnvPicker => app.handle_env_picker_key(key),
        InputMode::Selector => crate::app::selector::handle_key(app, key),
        InputMode::ProdConfirm => crate::app::prod_confirm::handle_key(app, key).await?,
        InputMode::UndoHistory => crate::undo::screen::handle_key(app, key),
        InputMode::Esv(mode) => crate::esv::screen::handle_key(app, key, mode)?,
        InputMode::Secrets(mode) => crate::secrets::screen::handle_key(app, key, mode)?,
        InputMode::Scripts(mode) => crate::scripts::screen::handle_key(app, key, mode),
        InputMode::Managed(mode) => crate::managed::screen::handle_key(app, key, mode),
        InputMode::Mappings(mode) => crate::mappings::screen::handle_key(app, key, mode),
        InputMode::Access(mode) => crate::access::screen::handle_key(app, key, mode),
        InputMode::IdmStore(mode) => crate::idmstore::screen::handle_key(app, key, mode),
        InputMode::Oauth(mode) => crate::oauth::screen::handle_key(app, key, mode),
        InputMode::Secretmap(mode) => crate::secretmap::screen::handle_key(app, key, mode),
    }
    Ok(())
}

/// Dispatch a Normal-mode key through the table. Returns without effect if no
/// binding matches.
pub async fn dispatch_normal(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    let act = Bind::resolve(&normal_binds(app), &key);
    if let Some(act) = act {
        run_normal(app, act).await;
    }
    Ok(())
}

async fn run_normal(app: &mut App, act: Act) {
    use Act::*;
    match act {
        Quit => app.should_quit = true,
        Functions => crate::app::selector::open(app),
        NextView => switch_esv_view(app, 1),
        PrevView => switch_esv_view(app, -1),
        Search => {
            if app.active_view == View::Esvs
                && crate::esv::screen::current_view(app) == EsvView::Mappings
            {
                crate::secretmap::screen::start_search(app);
            } else {
                app.input_mode = search_mode(app.active_view);
            }
        }
        ClearFilter => clear_filter(app),
        MoveDown => move_selection(app, 1),
        MoveUp => move_selection(app, -1),
        Top => set_selection(app, 0),
        Bottom => set_selection(app, usize::MAX),
        PageDown => move_selection(app, 10),
        PageUp => move_selection(app, -10),
        Primary => primary(app),
        Delete => delete(app),
        NewItem => new_item(app),
        RenameField => crate::managed::screen::start_rename_field(app),
        RenameObject => crate::managed::screen::start_rename_object(app),
        DeleteObject => crate::managed::screen::start_delete_object(app),
        NewObject => crate::managed::screen::start_new_object(app),
        AddHook => crate::managed::screen::start_add_hook(app),
        Pull => crate::scripts::screen::pull_selected(app),
        Push => crate::scripts::screen::push_selected(app),
        PullAll => crate::scripts::screen::pull_all(app),
        ReconMapping => crate::mappings::screen::run_recon(app),
        PullMappingScripts => crate::mappings::screen::pull_scripts(app),
        Apply => crate::esv::ops::request_restart(app),
        Refresh => {
            crate::app::refresh_view(app, app.active_view, true);
        }
        Undo => {
            let executor = if app.active_view == View::Access {
                crate::undo::UndoExecutor::Access
            } else if app.active_view == View::Managed {
                crate::undo::UndoExecutor::Managed
            } else if app.active_view == View::Esvs
                && crate::esv::screen::current_view(app) == EsvView::Mappings
            {
                crate::undo::UndoExecutor::SecretMapping
            } else {
                crate::undo::UndoExecutor::Esv
            };
            crate::undo::screen::request_latest(app, executor);
        }
        UndoHistory => {
            app.undo_history_idx = 0;
            app.input_mode = InputMode::UndoHistory;
        }
        PrevField => crate::managed::screen::move_property(app, -1),
        NextField => crate::managed::screen::move_property(app, 1),
        DetailScrollDown => scroll_active_detail(app, 10),
        DetailScrollUp => scroll_active_detail(app, -10),
        RealmToggle => {
            app.current_realm = match app.current_realm {
                Realm::Alpha => Realm::Bravo,
                Realm::Bravo => Realm::Alpha,
            };
        }
        TenantPicker => {
            if !app.tenants.is_empty() {
                app.env_picker_idx = app.active_tenant_idx;
                app.input_mode = InputMode::EnvPicker;
            }
        }
        Onboard => {
            app.onboard.menu_idx = 0;
            app.input_mode = InputMode::Onboard(OnboardMode::Menu);
        }
        AuthSettings => crate::vault::settings::open(app),
        Lock => crate::vault::unlock::lock_and_quit(app).await,
        Relock => crate::vault::unlock::open_relock(app),
    }
}

/// Route a detail-pane scroll to whichever pane is on screen. One function for
/// both directions, so a pane cannot be wired up for `^D` and forgotten for
/// `^U` — the delta is the only thing that differs.
fn scroll_active_detail(app: &mut App, delta: isize) {
    // Exhaustive, like the list-operation helpers below and unlike a guard with
    // a `_` fallback: a new `View` or `EsvView` with a scrollable detail pane
    // should be a compile error here, not a silently unbound key.
    match app.active_view {
        View::Access => crate::access::screen::scroll_detail(app, delta),
        View::Oauth => crate::oauth::screen::scroll_detail(app, delta),
        View::Scripts | View::Managed | View::Mappings | View::IdmStore => {}
        View::Esvs => match crate::esv::screen::current_view(app) {
            EsvView::Mappings => crate::secretmap::screen::scroll_detail(app, delta),
            EsvView::Variables | EsvView::Secrets => {}
        },
    }
}

fn mappings_allowed(app: &App) -> bool {
    app.active_tenant()
        .is_some_and(|tenant| tenant.allows_secret_mappings())
}

fn switch_esv_view(app: &mut App, delta: isize) {
    if app.active_view != View::Esvs {
        return;
    }
    let allowed = mappings_allowed(app);
    let next = if delta < 0 {
        app.esv.view.prev(allowed)
    } else {
        app.esv.view.next(allowed)
    };
    app.esv.view = next;
    if next == EsvView::Mappings && allowed {
        crate::secretmap::screen::refresh(app, false);
    }
}

fn row_count(app: &App) -> usize {
    match app.active_view {
        View::Scripts => crate::scripts::screen::row_count(app),
        View::Managed => crate::managed::screen::row_count(app),
        View::Mappings => crate::mappings::screen::row_count(app),
        View::Access => crate::access::screen::row_count(app),
        View::IdmStore => crate::idmstore::screen::row_count(app),
        View::Oauth => crate::oauth::screen::row_count(app),
        View::Esvs => match crate::esv::screen::current_view(app) {
            EsvView::Variables => crate::esv::screen::row_count(app),
            EsvView::Secrets => crate::secrets::screen::row_count(app),
            EsvView::Mappings => crate::secretmap::screen::row_count(app),
        },
    }
}

fn current_selection(app: &App) -> usize {
    match app.active_view {
        View::Scripts => crate::scripts::screen::current_selection(app),
        View::Managed => crate::managed::screen::current_selection(app),
        View::Mappings => crate::mappings::screen::current_selection(app),
        View::Access => crate::access::screen::current_selection(app),
        View::IdmStore => crate::idmstore::screen::current_selection(app),
        View::Oauth => crate::oauth::screen::current_selection(app),
        View::Esvs => match crate::esv::screen::current_view(app) {
            EsvView::Variables => crate::esv::screen::current_selection(app),
            EsvView::Secrets => crate::secrets::screen::current_selection(app),
            EsvView::Mappings => crate::secretmap::screen::current_selection(app),
        },
    }
}

fn set_selection(app: &mut App, idx: usize) {
    let clamped = idx.min(row_count(app).saturating_sub(1));
    match app.active_view {
        View::Scripts => crate::scripts::screen::set_selection(app, clamped),
        View::Managed => crate::managed::screen::set_selection(app, clamped),
        View::Mappings => crate::mappings::screen::select(app, clamped),
        View::Access => crate::access::screen::select(app, clamped),
        View::IdmStore => crate::idmstore::screen::select(app, clamped),
        View::Oauth => crate::oauth::screen::select(app, clamped),
        View::Esvs => match crate::esv::screen::current_view(app) {
            EsvView::Variables => crate::esv::screen::set_selection(app, clamped),
            EsvView::Secrets => crate::secrets::screen::set_selection(app, clamped),
            EsvView::Mappings => crate::secretmap::screen::select(app, clamped),
        },
    }
}

/// Move the current view's cursor by `delta`, clamped to the row range.
pub fn move_selection(app: &mut App, delta: isize) {
    let n = row_count(app);
    if n == 0 {
        return;
    }
    let cur = current_selection(app) as isize;
    let next = (cur + delta).clamp(0, n as isize - 1) as usize;
    set_selection(app, next);
}

fn filter_active(app: &App) -> bool {
    match app.active_view {
        View::Scripts => crate::scripts::screen::filter_active(app),
        View::Managed => crate::managed::screen::filter_active(app),
        View::Mappings => crate::mappings::screen::filter_active(app),
        View::Access => crate::access::screen::filter_active(app),
        View::IdmStore => crate::idmstore::screen::filter_active(app),
        View::Oauth => crate::oauth::screen::filter_active(app),
        View::Esvs => match crate::esv::screen::current_view(app) {
            EsvView::Variables => crate::esv::screen::filter_active(app),
            EsvView::Secrets => crate::secrets::screen::filter_active(app),
            EsvView::Mappings => crate::secretmap::screen::filter_active(app),
        },
    }
}

fn clear_filter(app: &mut App) {
    match app.active_view {
        View::Scripts => crate::scripts::screen::clear_filter(app),
        View::Managed => crate::managed::screen::clear_filter(app),
        View::Mappings => crate::mappings::screen::clear_filter(app),
        View::Access => crate::access::screen::clear_filter(app),
        View::IdmStore => crate::idmstore::screen::clear_filter(app),
        View::Oauth => crate::oauth::screen::clear_filter(app),
        View::Esvs => match crate::esv::screen::current_view(app) {
            EsvView::Variables => crate::esv::screen::clear_filter(app),
            EsvView::Secrets => crate::secrets::screen::clear_filter(app),
            EsvView::Mappings => crate::secretmap::screen::clear_filter(app),
        },
    }
}

fn primary(app: &mut App) {
    match app.active_view {
        View::Scripts => crate::scripts::screen::primary(app),
        View::Managed => crate::managed::screen::primary(app),
        View::Mappings => crate::mappings::screen::primary(app),
        View::Access => crate::access::screen::primary(app),
        View::IdmStore => crate::idmstore::screen::primary(app),
        View::Oauth => crate::oauth::screen::primary(app),
        View::Esvs => match crate::esv::screen::current_view(app) {
            EsvView::Variables => crate::esv::screen::primary(app),
            EsvView::Secrets => crate::secrets::screen::primary(app),
            EsvView::Mappings => crate::secretmap::screen::primary(app),
        },
    }
}

fn delete(app: &mut App) {
    match app.active_view {
        View::Scripts => crate::scripts::screen::delete(app),
        View::Managed => crate::managed::screen::delete(app),
        View::Mappings => crate::mappings::screen::delete(app),
        View::Access => crate::access::screen::delete(app),
        View::IdmStore => crate::idmstore::screen::delete(app),
        View::Oauth => crate::oauth::screen::delete(app),
        View::Esvs => match crate::esv::screen::current_view(app) {
            EsvView::Variables => crate::esv::screen::delete(app),
            EsvView::Secrets => crate::secrets::screen::delete(app),
            EsvView::Mappings => crate::secretmap::screen::delete(app),
        },
    }
}

fn new_item(app: &mut App) {
    match app.active_view {
        View::Scripts => crate::scripts::screen::new_item(app),
        View::Managed => crate::managed::screen::new_item(app),
        View::Mappings => crate::mappings::screen::new_item(app),
        View::Access => crate::access::screen::new_item(app),
        View::IdmStore => crate::idmstore::screen::new_item(app),
        View::Oauth => crate::oauth::screen::new_item(app),
        View::Esvs => match crate::esv::screen::current_view(app) {
            EsvView::Variables => crate::esv::screen::new_item(app),
            EsvView::Secrets => crate::secrets::screen::new_item(app),
            EsvView::Mappings => crate::secretmap::screen::new_item(app),
        },
    }
}

fn search_mode(view: View) -> InputMode {
    match view {
        View::Scripts => InputMode::Scripts(crate::scripts::screen::Mode::Search),
        View::Managed => InputMode::Managed(crate::managed::screen::Mode::Search),
        View::Mappings => InputMode::Mappings(crate::mappings::screen::Mode::Search),
        View::Access => InputMode::Access(crate::access::screen::Mode::Search),
        View::IdmStore => InputMode::IdmStore(crate::idmstore::screen::Mode::Search),
        View::Oauth => InputMode::Oauth(crate::oauth::screen::Mode::Search),
        View::Esvs => InputMode::Esv(EsvMode::Search),
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use serde_json::json;

    use super::{Act, Bind, normal_binds};
    use crate::access::state::{Document, LoadState as AccessLoadState};
    use crate::app::{App, View};
    use crate::config::tenant::{Tenant, TenantTheme};
    use crate::esv::state::{EsvView, LoadState as EsvLoadState};
    use crate::managed::state::LoadState as ManagedLoadState;
    use crate::mappings::api::MappingSummary;
    use crate::mappings::state::LoadState as MappingsLoadState;
    use crate::oauth::state::LoadState as OauthLoadState;
    use crate::scripts::Kind;
    use crate::scripts::screen::LoadState as ScriptsLoadState;
    use crate::scripts::sync::{Candidate, LocalState};
    use crate::secretmap::api::Mapping as SecretMapping;
    use crate::secretmap::state::LoadState as SecretmapLoadState;

    #[derive(Clone, Copy)]
    struct Case {
        name: &'static str,
        view: View,
        esv_view: EsvView,
        populated: bool,
        key: KeyEvent,
        expected: Option<Act>,
    }

    fn tenant() -> Tenant {
        Tenant {
            name: "test".into(),
            base_url: "https://test.invalid".into(),
            theme: TenantTheme::Sandbox,
            sa_id: None,
            scopes: Vec::new(),
        }
    }

    fn app_for(case: Case) -> App {
        let mut app = App::for_test(vec![tenant()], case.view);
        app.esv.view = case.esv_view;
        if !case.populated {
            return app;
        }

        let tenant = "test".to_string();
        match (case.view, case.esv_view) {
            (View::Esvs, EsvView::Variables) => {
                app.esv.list.data.insert(
                    tenant,
                    EsvLoadState::Loaded(vec![json!({"_id": "esv-one"})]),
                );
            }
            (View::Esvs, EsvView::Secrets) => {
                app.secret.list.data.insert(
                    tenant,
                    EsvLoadState::Loaded(vec![json!({"_id": "esv-secret-one"})]),
                );
            }
            (View::Esvs, EsvView::Mappings) => {
                app.secretmap.data.insert(
                    tenant,
                    SecretmapLoadState::Loaded(vec![SecretMapping {
                        secret_id: "scripted-decision-node".into(),
                        alias: Some("esv-secret-one".into()),
                    }]),
                );
            }
            (View::Scripts, _) => {
                app.scripts.data.insert(
                    tenant,
                    ScriptsLoadState::Loaded(vec![Candidate {
                        kind: Kind::Am,
                        realm: Some("alpha".into()),
                        name: "script-one".into(),
                        local: LocalState::Clean,
                        is_default: false,
                        context: None,
                        evaluator_version: None,
                    }]),
                );
            }
            (View::Managed, _) => {
                app.managed.data.insert(
                    tenant,
                    ManagedLoadState::Loaded(json!({
                        "_id": "managed",
                        "objects": [{
                            "name": "alpha_user",
                            "schema": {"properties": {"mail": {"type": "string"}}}
                        }]
                    })),
                );
            }
            (View::Mappings, _) => {
                app.mappings.data.insert(
                    tenant,
                    MappingsLoadState::Loaded(vec![MappingSummary {
                        name: "users".into(),
                        source: "system/ldap/account".into(),
                        target: "managed/alpha_user".into(),
                        inline_script_count: 0,
                        queued_sync: None,
                    }]),
                );
            }
            (View::Access, _) => {
                app.access.data.insert(
                    tenant,
                    AccessLoadState::Loaded(
                        Document::from_value(crate::access::six_rule_fixture()).unwrap(),
                    ),
                );
            }
            (View::Oauth, _) => {
                app.oauth
                    .data
                    .insert(tenant, OauthLoadState::Loaded(vec!["client-one".into()]));
            }
            // The IDM-store screen currently reports no list rows, so it has
            // no populated fixture to install.
            (View::IdmStore, _) => {}
        }
        app
    }

    fn char_key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    fn code(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn normal_bind_reachability_matches_each_view() {
        use Act::*;
        use EsvView::{Mappings as EsvMappings, Secrets, Variables};
        use View::*;

        let mut cases = Vec::new();
        let mut add = |name, view, esv_view, populated, key, expected| {
            cases.push(Case {
                name,
                view,
                esv_view,
                populated,
                key,
                expected,
            });
        };

        // Removing or moving a view's primary/create/delete pushes in
        // `normal_binds` makes these rows red. Uppercase D on Access is pinned
        // alongside the existing d and Delete triggers.
        add(
            "variable edit",
            Esvs,
            Variables,
            true,
            code(KeyCode::Enter),
            Some(Primary),
        );
        add(
            "variable delete",
            Esvs,
            Variables,
            true,
            char_key('d'),
            Some(Delete),
        );
        add(
            "variable create",
            Esvs,
            Variables,
            true,
            ctrl('n'),
            Some(NewItem),
        );
        add(
            "secret versions",
            Esvs,
            Secrets,
            true,
            code(KeyCode::Enter),
            Some(Primary),
        );
        add(
            "secret delete",
            Esvs,
            Secrets,
            true,
            char_key('D'),
            Some(Delete),
        );
        add(
            "secret create",
            Esvs,
            Secrets,
            true,
            ctrl('n'),
            Some(NewItem),
        );
        add(
            "secret mapping edit",
            Esvs,
            EsvMappings,
            true,
            char_key('e'),
            Some(Primary),
        );
        add(
            "secret mapping delete",
            Esvs,
            EsvMappings,
            true,
            char_key('d'),
            Some(Delete),
        );
        add(
            "secret mapping add",
            Esvs,
            EsvMappings,
            true,
            char_key('a'),
            Some(NewItem),
        );
        add(
            "managed edit field",
            Managed,
            Variables,
            true,
            code(KeyCode::Enter),
            Some(Primary),
        );
        add(
            "managed add field",
            Managed,
            Variables,
            true,
            char_key('a'),
            Some(NewItem),
        );
        add(
            "managed delete field",
            Managed,
            Variables,
            true,
            char_key('d'),
            Some(Delete),
        );
        add(
            "access edit",
            Access,
            Variables,
            true,
            code(KeyCode::Enter),
            Some(Primary),
        );
        add(
            "access delete d",
            Access,
            Variables,
            true,
            char_key('d'),
            Some(Delete),
        );
        add(
            "access delete D",
            Access,
            Variables,
            true,
            char_key('D'),
            Some(Delete),
        );
        add(
            "access Delete",
            Access,
            Variables,
            true,
            code(KeyCode::Delete),
            Some(Delete),
        );
        add(
            "oauth inspect",
            Oauth,
            Variables,
            true,
            code(KeyCode::Enter),
            Some(Primary),
        );
        add(
            "mappings has no primary",
            Mappings,
            Variables,
            true,
            code(KeyCode::Enter),
            None,
        );
        add(
            "scripts has no delete",
            Scripts,
            Variables,
            true,
            char_key('d'),
            None,
        );
        add(
            "oauth has no delete",
            Oauth,
            Variables,
            true,
            char_key('d'),
            None,
        );

        // Deleting a view's ^Z/^Y pushes, or widening their view guard onto a
        // read-only/non-undoable tab, makes this group red.
        for (name, view, esv_view) in [
            ("variables", Esvs, Variables),
            ("secrets", Esvs, Secrets),
            ("secret mappings", Esvs, EsvMappings),
            ("managed", Managed, Variables),
            ("access", Access, Variables),
        ] {
            add(name, view, esv_view, true, ctrl('z'), Some(Undo));
            add(name, view, esv_view, true, ctrl('y'), Some(UndoHistory));
        }
        for (name, view) in [
            ("scripts cannot undo", Scripts),
            ("sync mappings cannot undo", Mappings),
            ("query cannot undo", IdmStore),
            ("oauth cannot undo", Oauth),
        ] {
            add(name, view, Variables, true, ctrl('z'), None);
            add(name, view, Variables, true, ctrl('y'), None);
        }

        // Headline regression guard: deleting the ^D/^U pushes from the
        // `access_view` branch of `normal_binds` must fail this test, because
        // that is the defect it exists to prevent. Moving detail scrolling to
        // a view without a detail pane makes the negative rows red.
        for (name, view, esv_view) in [
            ("secret mapping", Esvs, EsvMappings),
            ("access", Access, Variables),
            ("oauth", Oauth, Variables),
        ] {
            add(
                name,
                view,
                esv_view,
                true,
                ctrl('d'),
                Some(DetailScrollDown),
            );
            add(name, view, esv_view, true, ctrl('u'), Some(DetailScrollUp));
        }
        for (name, view, esv_view) in [
            ("variables have no detail scroll", Esvs, Variables),
            ("secrets have no detail scroll", Esvs, Secrets),
            ("scripts have no detail scroll", Scripts, Variables),
            ("managed has no detail scroll", Managed, Variables),
            ("sync mappings have no detail scroll", Mappings, Variables),
            ("query has no detail scroll", IdmStore, Variables),
        ] {
            add(name, view, esv_view, true, ctrl('d'), None);
            add(name, view, esv_view, true, ctrl('u'), None);
        }

        // Removing ^R from a refreshable view, or adding it to a view whose
        // refresh lifecycle uses another action, makes these rows red.
        for (name, view, esv_view) in [
            ("secret mappings refresh", Esvs, EsvMappings),
            ("managed refresh", Managed, Variables),
            ("sync mappings refresh", Mappings, Variables),
            ("access refresh", Access, Variables),
            ("query refresh", IdmStore, Variables),
            ("oauth refresh", Oauth, Variables),
        ] {
            add(name, view, esv_view, true, ctrl('r'), Some(Refresh));
        }
        for (name, view, esv_view) in [
            ("variables do not expose refresh", Esvs, Variables),
            ("secrets do not expose refresh", Esvs, Secrets),
            ("scripts do not expose refresh", Scripts, Variables),
        ] {
            add(name, view, esv_view, true, ctrl('r'), None);
        }

        // Removing any managed-only push, or letting one escape the managed
        // view guard, makes this group red.
        for (name, key, expected) in [
            ("rename field", char_key('r'), RenameField),
            ("rename object", char_key('R'), RenameObject),
            ("delete object", char_key('D'), DeleteObject),
            ("add hook", char_key('h'), AddHook),
            ("previous field", char_key('['), PrevField),
            ("next field", char_key(']'), NextField),
            ("new object", ctrl('n'), NewObject),
        ] {
            add(name, Managed, Variables, true, key, Some(expected));
        }
        add(
            "access has no rename object",
            Access,
            Variables,
            true,
            char_key('R'),
            None,
        );
        add(
            "oauth has no new object",
            Oauth,
            Variables,
            true,
            ctrl('n'),
            None,
        );

        // Access deliberately leaves ^N, ^Z, ^Y, and ^R outside `n > 0`, but
        // selection-dependent edit/delete/detail actions stay inside it.
        for (name, key, expected) in [
            ("create first access rule", ctrl('n'), Some(NewItem)),
            ("undo deletion of last access rule", ctrl('z'), Some(Undo)),
            ("empty access undo history", ctrl('y'), Some(UndoHistory)),
            ("refresh empty access", ctrl('r'), Some(Refresh)),
            ("no empty access edit", code(KeyCode::Enter), None),
            ("no empty access delete d", char_key('d'), None),
            ("no empty access delete D", char_key('D'), None),
            ("no empty access Delete", code(KeyCode::Delete), None),
            ("no empty access detail down", ctrl('d'), None),
            ("no empty access detail up", ctrl('u'), None),
        ] {
            add(name, Access, Variables, false, key, expected);
        }

        for case in &cases {
            let app = app_for(*case);
            let actual = Bind::resolve(&normal_binds(&app), &case.key);
            assert_eq!(
                actual, case.expected,
                "{}: {:?}/{:?}, populated={}, key={:?}",
                case.name, case.view, case.esv_view, case.populated, case.key
            );
        }

        for view in View::all() {
            assert!(
                cases.iter().any(|case| case.view == *view),
                "reachability table omitted {view:?}"
            );
        }
        for esv_view in [Variables, Secrets, EsvMappings] {
            assert!(
                cases
                    .iter()
                    .any(|case| case.view == Esvs && case.esv_view == esv_view),
                "reachability table omitted ESV sub-view {esv_view:?}"
            );
        }
    }
}
