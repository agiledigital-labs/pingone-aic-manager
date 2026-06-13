# aic-edit — Roadmap

Updated 2026-06-13. History lives in git — this file only tracks what's
done, what's in flight, and what's next. (Earlier revisions of this file
held the full Step 1–6 implementation narratives; see git history if you
need the archaeology.)

## Done

- **API research** — verified reference in `docs/api/` + the
  `scripts/verify-endpoint.sh` verify-first loop.
- **TUI foundation** — unlock/vault (Argon2id master password and/or FIDO2
  hmac-secret security key, DEK envelope in `wraps.toml`), onboarding
  (cookie / userpass / paste / sandbox import), env picker with per-env
  themes, prod-write guard, undo log + history screen.
- **Agent + CLI** — single `aic` binary; `ssh-agent`-shaped daemon owns
  JWKs, token cache, and the HTTP pool; all tenant HTTP from both surfaces
  goes through `aic::api`.
- **ESVs** — list / fuzzy search / edit / delete / apply (restart) for
  variables, full secrets lifecycle (versions, enable/disable/destroy),
  TUI tab + `aic esv` CLI.
- **Scripts** — typed local workspace (`.d.ts` + ESLint, runtime-verified
  against Rhino 1.7.14), pull/push/sync/watch/status/diff with
  content-based conflict detection, AM scripts + IDM endpoints + IDM
  schedules, TUI tab + `aic script` CLI.

- **Feature-vertical restructure** (2026-06-13) — one directory per feature
  (`esv/`, `secrets/`, `scripts/`, `onboard/`, `vault/`, `undo/`) with
  uniform api/state/ops/screen/view/cli seams, nested per-feature
  Mode/Event enums, `app/` as the only global glue and `tui/` as passive
  chrome. Rationale + phase log:
  [`docs/orthogonality-review.md`](docs/orthogonality-review.md); routing
  map: CLAUDE.md §9.

## Next

- **OAuth2 / OIDC** — clients + provider service (`docs/api/05-oauth2-oidc.md`).
  Remember: strip `-encrypted` fields on PUT; use `_rev` + content snapshot.
- **SAML** — hosted/remote entities + CoT (`docs/api/06-saml.md`).
- **Journeys** — read-only browse first (`docs/api/09-journeys.md`).

## Parked / stretch

- **Log sync + search** — offline history past AIC's 30-day retention
  (`docs/api/08-logs.md`; separate API-key auth).
- **Browser-handoff onboarding** for SSO-only admins — blocked by AIC
  platform limitations; see `docs/api/99-quirks-and-open-questions.md`
  Q11/Q12 for the dead ends already explored.
