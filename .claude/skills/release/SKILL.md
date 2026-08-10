---
name: release
description:
  Cut a new `aic` release — check readiness, pick the version bump, write the
  release notes, and publish. Use when asked to release, tag, mint a version, or
  add release notes for pingone-aic-manager.
---

# Release `aic`

Two scripts do the mechanical work. You do the three things that need judgement:
**pick the version**, **write the notes**, **check the results**.

```bash
scripts/release-check.sh                          # readiness + what's in the release
scripts/release.sh <version> <notes-file>         # bump, commit, tag, push, publish
```

`release-check.sh` verifies branch/tree/sync/gh and runs the full gate suite
(fmt, clippy, test), staying quiet unless something fails. On success it prints
the current version, the last tag, the commit log since it, and a per-area file
summary — that's your review material.

`release.sh` validates everything before its first mutation, so a bad invocation
can't leave a half-release behind. Add `--dry-run` to stop after validation. If
a step past the bump fails, it prints the recovery commands.

## Your part

1. Run `scripts/release-check.sh`. If it fails, fix the cause — don't work
   around it.
2. **Read the commit log it printed** and review anything you didn't write
   yourself. The scripts check that the code builds, not that it should ship.
3. **Pick the version** (policy below). State the choice and the reason.
4. **Write the notes** to a scratchpad file (guidance below).
5. Run `scripts/release.sh <version> <notes-file>`, then confirm the release URL
   it prints.

## Version policy (pre-1.0)

Default to a **patch** bump (`0.3.0` → `0.3.1`).

Bump the **minor** (`0.3.0` → `0.4.0`) only for:

- a **major new feature** — a new tab, a new CLI noun, a new capability someone
  would come looking for. Not a batch of refinements to existing ones, however
  large; if every busy cycle argues itself into a minor, the distinction stops
  carrying information.
- **any breaking change** — a removed or renamed command or flag, a changed
  default, or a previously-working operation that now refuses (e.g. the
  staging/production script-write guard in v0.3.0).
- **a required agent restart** — any release that bumps `PROTOCOL_VERSION` in
  `src/agent/mod.rs`, because every user must run `aic session stop` before the
  new CLI will talk to their resident daemon. Grep the diff for it; it is easy
  to miss in someone else's commit. The version number is how an operator knows
  a restart is coming without reading the notes, so this is not negotiable even
  when the rest of the release is small (policy set 2026-08-10).

Never bump the major while pre-1.0.

When it's genuinely borderline, say which way you're leaning and why, then ask.
A wrong patch is cheap to follow with a minor; a wrong minor is published.

## Writing the notes

The notes file becomes **both** the annotated tag message and the GitHub release
body — write it once.

- **Lead with a "Heads-up before you upgrade" section** whenever the release
  changes behaviour someone already depends on: a refusal where a write used to
  succeed, a changed default, a flag that no longer does anything. Say what
  changed, why, and what still works. If there's nothing to warn about, omit the
  section rather than padding it.
- Then group by area — Script lifecycle, Managed objects, Sync, Agent, Script
  workspace typing, CLI. Use whichever apply.
- Describe what a user can now do, not which commits landed. No hashes, no
  `feat(x):` prefixes, no contributor list (single-author repo).
- Add a **Verified API corrections** section when the cycle contradicted a claim
  in `docs/api/`. Those change how people reason about the tenant, so they
  belong in the notes and not just the doc.

Patch releases carry the same weight here: with patch-by-default, `0.3.1` is
indistinguishable from a one-line fix unless the notes say otherwise.

## Gotchas

- `v0.1.0`–`v0.2.1` are **lightweight** tags with **empty** GitHub release
  bodies. Leave them alone — retagging published refs breaks anyone who has
  fetched them. `v0.3.0` onward are annotated with written notes.
- The version lives only in `Cargo.toml` (+ the lock). No CHANGELOG file, no
  version constant in the source.
- Don't confuse this with `TEMPLATES_VERSION` in `src/scripts/workspace.rs` —
  that tracks script-workspace templates and moves independently of releases.
