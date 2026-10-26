//! `aic script diff <ref> --find-in-git <repo>`: which branch heads hold the
//! tenant's bytes for a script, byte for byte.
//!
//! The question is "whose change is this?" when the tenant differs from the
//! workspace. `script who` names the account that wrote it; this names the
//! branch heads holding the same bytes, by asking git for the blob id of the
//! tenant's bytes and looking for that id in every branch head's whole tree.
//! Identical bytes, not provenance: two branches can hold the same file. Exact
//! bytes only — a bundle rebuilt with another toolchain, or source whose
//! trailing newline differs, is a different blob and is reported as not found.
//!
//! Local git only: `hash-object`, `for-each-ref`, `ls-tree`, in an
//! environment that pins the repository to the one named and forbids the two
//! ways git can answer from somewhere else — a partial clone's lazy fetch, and
//! replacement objects (round 8, both reviewers).

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::{Error, Result};

/// One branch head whose tree holds the blob.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// `refs/heads/<name>` or `refs/remotes/<remote>/<name>`.
    pub reference: String,
    pub commit: String,
    /// The path, with any byte that is not UTF-8 shown as U+FFFD.
    pub path: String,
    /// A symlink whose target text is these bytes, not a file holding them.
    pub symlink: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Search {
    /// The blob id the bytes hash to in this repository's object format.
    pub blob: String,
    /// How many branch heads were searched.
    pub searched: usize,
    pub hits: Vec<Hit>,
}

/// Variables that would make git read a repository other than `-C <repo>`,
/// inherited from a hook or a wrapper (round 8: an inherited `GIT_DIR` made
/// `git -C /tmp` answer from this checkout).
const ROUTING: [&str; 8] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_COMMON_DIR",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_NAMESPACE",
    "GIT_PREFIX",
];

fn git(repo: &Path) -> Command {
    let mut command = Command::new("git");
    for name in ROUTING {
        command.env_remove(name);
    }
    command
        .arg("--no-replace-objects")
        .arg("-C")
        .arg(repo)
        // A partial clone would otherwise fetch a missing tree from its
        // remote: answer from what is here, or fail saying so.
        .env("GIT_NO_LAZY_FETCH", "1")
        // A hang is the one failure no error handling catches, so make sure
        // nothing can stop to ask for credentials.
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn run(mut command: Command, input: Option<&[u8]>, what: &str) -> Result<Vec<u8>> {
    if input.is_some() {
        command.stdin(Stdio::piped());
    }
    let mut child = command.spawn().map_err(|error| {
        Error::Config(format!(
            "couldn't run `git` for {what} ({error}) — is git on your PATH?"
        ))
    })?;
    if let Some(input) = input {
        child
            .stdin
            .take()
            .expect("stdin was piped above")
            .write_all(input)?;
    }
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(Error::Config(format!(
            "`git {what}` failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(output.stdout)
}

/// `for-each-ref --format='%(objectname) %(symref) %(refname)'` output →
/// (commit, ref), leaving out symbolic refs (`origin/HEAD`), which alias a
/// branch already listed. Decided by `%(symref)`, not the name: a real
/// branch may be called `release/HEAD` (round 8).
fn parse_refs(output: &str) -> Vec<(String, String)> {
    output
        .lines()
        .filter_map(|line| {
            let (commit, rest) = line.split_once(' ')?;
            let (symref, reference) = rest.split_once(' ')?;
            symref
                .is_empty()
                .then(|| (commit.to_string(), reference.to_string()))
        })
        .collect()
}

/// `ls-tree -r -z` output → (path, symlink) for each entry whose object id is
/// `blob`. Entries are `<mode> SP <type> SP <oid> TAB <path> NUL`; parsed as
/// bytes, so a path that is not UTF-8 is shown lossily rather than dropped.
fn paths_holding(output: &[u8], blob: &str) -> Vec<(String, bool)> {
    output
        .split(|byte| *byte == 0)
        .filter_map(|entry| {
            let tab = entry.iter().position(|byte| *byte == b'\t')?;
            let meta = std::str::from_utf8(&entry[..tab]).ok()?;
            let mut fields = meta.split(' ');
            let (mode, kind, oid) = (fields.next()?, fields.next()?, fields.next()?);
            (kind == "blob" && oid == blob).then(|| {
                (
                    String::from_utf8_lossy(&entry[tab + 1..]).into_owned(),
                    mode == "120000",
                )
            })
        })
        .collect()
}

/// Every branch head in `repo` — local and remote-tracking — whose whole tree
/// holds exactly `bytes`. Each distinct commit is listed once, however many
/// heads point at it.
pub fn find(repo: &Path, bytes: &[u8]) -> Result<Search> {
    let mut hash = git(repo);
    hash.args(["hash-object", "--stdin"]);
    let blob = String::from_utf8_lossy(&run(hash, Some(bytes), "hash-object")?)
        .trim()
        .to_string();
    let mut refs = git(repo);
    refs.args([
        "for-each-ref",
        "--format=%(objectname) %(symref) %(refname)",
        "refs/heads",
        "refs/remotes",
    ]);
    let refs = parse_refs(&String::from_utf8_lossy(&run(refs, None, "for-each-ref")?));
    let mut listed: HashMap<&str, Vec<(String, bool)>> = HashMap::new();
    let mut hits = Vec::new();
    for (commit, reference) in &refs {
        if !listed.contains_key(commit.as_str()) {
            let mut tree = git(repo);
            // `--full-tree`: a REPO that is a subdirectory still searches the
            // whole tree, not the part below it (round 8).
            tree.args(["ls-tree", "-r", "-z", "--full-tree", commit]);
            let paths = paths_holding(&run(tree, None, "ls-tree")?, &blob);
            listed.insert(commit, paths);
        }
        for (path, symlink) in &listed[commit.as_str()] {
            hits.push(Hit {
                reference: reference.clone(),
                commit: commit.clone(),
                path: path.clone(),
                symlink: *symlink,
            });
        }
    }
    Ok(Search {
        blob,
        searched: refs.len(),
        hits,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_blobs_with_the_exact_id_match() {
        let listing = b"100644 blob abc\tidm/endpoint/x.cjs\0\
            100644 blob abd\tidm/endpoint/y.cjs\0\
            160000 commit abc\tvendor/sub\0\
            120000 blob abc\tlink.cjs\0\
            100644 blob abc\tpath with\ttab.cjs\0\
            100644 blob abc\tbad\xffname.cjs\0";
        assert_eq!(
            paths_holding(listing, "abc"),
            vec![
                ("idm/endpoint/x.cjs".to_string(), false),
                ("link.cjs".to_string(), true),
                ("path with\ttab.cjs".to_string(), false),
                ("bad\u{fffd}name.cjs".to_string(), false),
            ]
        );
    }

    fn git_in(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args([
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=/dev/null",
            ])
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }

    /// Drives the real `find` against a throwaway repository: the exact bytes
    /// are found on the branch that has them and only there; a one-byte
    /// difference is not found anywhere. A branch named `…/HEAD` is a real
    /// branch, a symbolic ref is not searched twice, and a REPO that is a
    /// subdirectory still searches the whole tree.
    #[test]
    fn finds_the_branch_holding_the_exact_bytes() {
        let dir = std::env::temp_dir().join(format!("aic-gitfind-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let sh = |args: &[&str]| git_in(&dir, args);
        sh(&["init", "-q", "-b", "main"]);
        std::fs::create_dir_all(dir.join("other")).unwrap();
        std::fs::write(dir.join("other/a.cjs"), "one\n").unwrap();
        sh(&["add", "other"]);
        sh(&["commit", "-q", "-m", "one"]);
        sh(&["checkout", "-q", "-b", "release/HEAD"]);
        std::fs::create_dir_all(dir.join("idm/endpoint")).unwrap();
        std::fs::write(dir.join("idm/endpoint/x.cjs"), "two\n").unwrap();
        sh(&["add", "idm"]);
        sh(&["commit", "-q", "-m", "two"]);
        sh(&["symbolic-ref", "refs/heads/alias", "refs/heads/main"]);

        let found = find(&dir, b"two\n").unwrap();
        assert_eq!(found.searched, 2, "main and release/HEAD; not the alias");
        assert_eq!(found.hits.len(), 1);
        assert_eq!(found.hits[0].reference, "refs/heads/release/HEAD");
        assert_eq!(found.hits[0].path, "idm/endpoint/x.cjs");
        let from_subdir = find(&dir.join("other"), b"two\n").unwrap();
        assert_eq!(from_subdir.hits, found.hits);
        assert!(find(&dir, b"two").unwrap().hits.is_empty());
        assert!(find(&dir.join("missing"), b"two\n").is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A local `git replace` must not make a head's tree look like another
    /// commit's: `main` is at C; D (on no branch) replaces C and holds bytes C
    /// does not. The control is git without `--no-replace-objects`.
    #[test]
    fn replacement_objects_are_ignored() {
        let dir = std::env::temp_dir().join(format!("aic-gitrepl-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let sh = |args: &[&str]| git_in(&dir, args);
        sh(&["init", "-q", "-b", "main"]);
        std::fs::write(dir.join("x.cjs"), "one\n").unwrap();
        sh(&["add", "x.cjs"]);
        sh(&["commit", "-q", "-m", "c"]);
        sh(&["checkout", "-q", "--detach"]);
        std::fs::write(dir.join("x.cjs"), "replaced\n").unwrap();
        sh(&["commit", "-q", "-am", "d"]);
        sh(&["replace", "main", "HEAD"]);
        let control = Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(["show", "main:x.cjs"])
            .output()
            .unwrap();
        assert_eq!(
            control.stdout, b"replaced\n",
            "the control: replace is live"
        );
        assert!(find(&dir, b"replaced\n").unwrap().hits.is_empty());
        assert_eq!(find(&dir, b"one\n").unwrap().hits.len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A treeless partial clone missing a head's tree fails instead of
    /// fetching it. The control, after: plain git fetches the tree, and only
    /// then does `find` answer.
    #[test]
    fn a_partial_clone_does_not_fetch() {
        let base = std::env::temp_dir().join(format!("aic-gitlazy-{}", uuid::Uuid::new_v4()));
        let (origin, clone) = (base.join("origin"), base.join("clone"));
        std::fs::create_dir_all(&origin).unwrap();
        git_in(&origin, &["init", "-q", "-b", "main"]);
        git_in(&origin, &["config", "uploadpack.allowfilter", "true"]);
        std::fs::write(origin.join("x.cjs"), "two\n").unwrap();
        git_in(&origin, &["add", "x.cjs"]);
        git_in(&origin, &["commit", "-q", "-m", "two"]);
        git_in(
            &base,
            &[
                "clone",
                "-q",
                "--filter=tree:0",
                "--no-checkout",
                &format!("file://{}", origin.display()),
                "clone",
            ],
        );
        assert!(find(&clone, b"two\n").is_err());
        git_in(&clone, &["ls-tree", "-r", "main"]);
        assert_eq!(
            find(&clone, b"two\n").unwrap().hits.len(),
            2,
            "main, origin/main"
        );
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// An inherited `GIT_DIR` redirects git away from `-C` — the control,
    /// run through `env` so the variable is really inherited — and `git()`
    /// removes it and every other routing variable from what it runs.
    #[test]
    fn an_inherited_git_dir_is_removed() {
        let base = std::env::temp_dir().join(format!("aic-gitdir-{}", uuid::Uuid::new_v4()));
        let (holder, empty) = (base.join("holder"), base.join("empty"));
        for dir in [&holder, &empty] {
            std::fs::create_dir_all(dir).unwrap();
            git_in(dir, &["init", "-q", "-b", "main"]);
        }
        std::fs::write(holder.join("x.cjs"), "two\n").unwrap();
        git_in(&holder, &["add", "x.cjs"]);
        git_in(&holder, &["commit", "-q", "-m", "two"]);
        let control = Command::new("env")
            .arg(format!("GIT_DIR={}", holder.join(".git").display()))
            .args(["git", "-C"])
            .arg(&empty)
            .args(["for-each-ref", "--format=%(refname)"])
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&control.stdout).contains("refs/heads/main"),
            "the control: inherited GIT_DIR wins over -C"
        );
        let removed: Vec<String> = git(&empty)
            .get_envs()
            .filter(|(_, value)| value.is_none())
            .map(|(name, _)| name.to_string_lossy().into_owned())
            .collect();
        for name in ROUTING {
            assert!(removed.iter().any(|r| r == name), "{name} not removed");
        }
        std::fs::remove_dir_all(&base).unwrap();
    }
}
