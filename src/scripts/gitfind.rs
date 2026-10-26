//! `aic script diff <ref> --find-in-git <repo>`: which branch heads hold the
//! tenant's bytes for a script, byte for byte.
//!
//! The question is "whose change is this?" when the tenant differs from the
//! workspace. `script who` names the account that wrote it; this names the
//! branch it came from, by asking git for the blob id of the tenant's bytes and
//! looking for that id in every branch head's tree. Exact bytes only — a bundle
//! rebuilt with another toolchain, or source whose trailing newline differs,
//! is a different blob and is reported as not found.
//!
//! Local git only: `hash-object`, `for-each-ref`, `ls-tree`. Nothing fetches,
//! so the answer is as fresh as the clone, and the output says which refs were
//! searched.

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
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Search {
    /// The blob id the bytes hash to in this repository's object format.
    pub blob: String,
    /// How many branch heads were searched.
    pub searched: usize,
    pub hits: Vec<Hit>,
}

fn git(repo: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(repo)
        // Every call is local, but a hang is the one failure no error handling
        // catches, so make sure nothing can stop to ask for credentials.
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

/// `for-each-ref --format='%(objectname) %(refname)'` output → (commit, ref),
/// leaving out symbolic `…/HEAD` entries, which name a branch already listed.
fn parse_refs(output: &str) -> Vec<(String, String)> {
    output
        .lines()
        .filter_map(|line| line.split_once(' '))
        .filter(|(_, reference)| !reference.ends_with("/HEAD"))
        .map(|(commit, reference)| (commit.to_string(), reference.to_string()))
        .collect()
}

/// `ls-tree -r -z` output → the paths whose object id is `blob`. Entries are
/// `<mode> SP <type> SP <oid> TAB <path> NUL`; `-z` keeps paths verbatim.
fn paths_holding(output: &[u8], blob: &str) -> Vec<String> {
    output
        .split(|byte| *byte == 0)
        .filter_map(|entry| {
            let entry = std::str::from_utf8(entry).ok()?;
            let (meta, path) = entry.split_once('\t')?;
            let mut fields = meta.split(' ');
            let (_mode, kind, oid) = (fields.next()?, fields.next()?, fields.next()?);
            (kind == "blob" && oid == blob).then(|| path.to_string())
        })
        .collect()
}

/// Every branch head in `repo` — local and remote-tracking — whose tree
/// holds exactly `bytes`.
pub fn find(repo: &Path, bytes: &[u8]) -> Result<Search> {
    let mut hash = git(repo);
    hash.args(["hash-object", "--stdin"]);
    let blob = String::from_utf8_lossy(&run(hash, Some(bytes), "hash-object")?)
        .trim()
        .to_string();
    let mut refs = git(repo);
    refs.args([
        "for-each-ref",
        "--format=%(objectname) %(refname)",
        "refs/heads",
        "refs/remotes",
    ]);
    let refs = parse_refs(&String::from_utf8_lossy(&run(refs, None, "for-each-ref")?));
    let mut hits = Vec::new();
    for (commit, reference) in &refs {
        let mut tree = git(repo);
        tree.args(["ls-tree", "-r", "-z", commit]);
        for path in paths_holding(&run(tree, None, "ls-tree")?, &blob) {
            hits.push(Hit {
                reference: reference.clone(),
                commit: commit.clone(),
                path,
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
    fn symbolic_heads_are_skipped() {
        let refs = parse_refs(
            "aaa refs/heads/main\nbbb refs/remotes/origin/HEAD\nccc refs/remotes/origin/feature\n",
        );
        assert_eq!(
            refs,
            vec![
                ("aaa".to_string(), "refs/heads/main".to_string()),
                ("ccc".to_string(), "refs/remotes/origin/feature".to_string()),
            ]
        );
    }

    #[test]
    fn only_blobs_with_the_exact_id_match() {
        let listing = b"100644 blob abc\tidm/endpoint/x.cjs\0\
            100644 blob abd\tidm/endpoint/y.cjs\0\
            160000 commit abc\tvendor/sub\0\
            100644 blob abc\tpath with\ttab.cjs\0";
        assert_eq!(
            paths_holding(listing, "abc"),
            vec![
                "idm/endpoint/x.cjs".to_string(),
                "path with\ttab.cjs".to_string()
            ]
        );
    }

    /// Drives the real `find` against a throwaway repository: the exact bytes
    /// are found on the branch that has them and only there; a one-byte
    /// difference is not found anywhere.
    #[test]
    fn finds_the_branch_holding_the_exact_bytes() {
        let dir = std::env::temp_dir().join(format!("aic-gitfind-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let sh = |args: &[&str]| {
            let status = Command::new("git")
                .arg("-C")
                .arg(&dir)
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
        };
        sh(&["init", "-q", "-b", "main"]);
        std::fs::write(dir.join("a.cjs"), "one\n").unwrap();
        sh(&["add", "a.cjs"]);
        sh(&["commit", "-q", "-m", "one"]);
        sh(&["checkout", "-q", "-b", "feature"]);
        std::fs::create_dir_all(dir.join("idm/endpoint")).unwrap();
        std::fs::write(dir.join("idm/endpoint/x.cjs"), "two\n").unwrap();
        sh(&["add", "idm"]);
        sh(&["commit", "-q", "-m", "two"]);

        let found = find(&dir, b"two\n").unwrap();
        assert_eq!(found.searched, 2);
        assert_eq!(found.hits.len(), 1);
        assert_eq!(found.hits[0].reference, "refs/heads/feature");
        assert_eq!(found.hits[0].path, "idm/endpoint/x.cjs");
        assert!(find(&dir, b"two").unwrap().hits.is_empty());
        assert!(find(&dir.join("missing"), b"two\n").is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
