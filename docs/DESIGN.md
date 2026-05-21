# aic-edit — TUI design rules

These are the agreed visual + interaction rules for `aic-edit`. Apply them
whenever building or reviewing UI code. They were arrived at by reviewing two
existing Ratatui apps the maintainer wrote (`tally`, `kbsr`) and a short
design conversation that pinned down env chrome, issue surfacing, and
keybindings. Don't redebate these unless the maintainer explicitly revisits.

## Borders, separators, palette

- **No borders on main panels.** Borders only on confirmation/error modals.
- Use whitespace, position, and color for hierarchy. Two-space tab dividers.
- Transparent backgrounds. Selection = `bg(DarkGray)` on the row (no `>` glyph).
- Labels in `DarkGray`. Values in `White` (or semantic color).
- Semantic colors: red=out/error, green=in/ok, yellow=action/caution,
  cyan=highlight/key-hints, blue=informational, magenta=rare-special.

## Navigation

- **Top tab strip (tally-style):** e.g. `ESVs  Scripts  OAuth2  SAML  Logs`.
  Active tab bold-white; inactive DarkGray. Tab/Shift+Tab to move; number
  keys jump.

## Within-tab layout

- **Yazi-style miller** for rich content tabs (Scripts, OAuth2, SAML): list
  on the left (~30%), detail/editor on the right. Always visible. Enter
  focuses the right pane.
- For simple list+short-detail tabs (ESVs), tally's list-top + fixed-detail-strip
  pattern is fine.

## Header chrome (right-aligned)

- **Realm chip** + **Env chip**. Realm chip is dim (`[alpha]` or `[bravo]`).
- `R` toggles realm (always alpha ↔ bravo).
- `T` opens an environment picker modal (centered, borderless except for the
  modal frame).
- Env chip uses the environment's theme color:
  - **sandbox** → green
  - **development** (also predev) → blue
  - **staging (also UAT)** → yellow
  - **production** → red + `⚠`
- Out-of-the-box environments: dev, staging, prod. Sandbox is the next
  most-commonly-added. Extra environments (e.g. predev, UAT) pick one of
  the four themes.

## Issue surfacing

- **Inline strip** at the top of the relevant pane for actionable state
  (e.g. yellow strip "⚠ restart pending — 3 ESVs changed [a] apply" in
  the ESV pane; red strip "push conflict — remote drifted" in the Scripts
  pane).
- **Top-right toast overlay** for transient events (token refreshed, push
  succeeded, fetched 47 scripts). Auto-dismiss; stack vertically.

## Keybindings

- Vim-like: `j`/`k`, `h`/`l`, `gg`/`G`. `/` for search. `Esc` always
  cancels. `Enter` confirms or focuses. Single-char actions (`a` apply,
  `e` edit, `d` delete, `q` quit, `R` realm, `T` tenant).
- No Ctrl combos for common ops (mirrors tally).
- Mouse capture on but not relied on (mirrors tally).

## Modals

- Used sparingly: env picker, confirm-destructive, push-conflict resolver,
  error reporter. Rendered with `Clear` + black bg; border + title only when
  the modal is a _choice_ (env picker, conflict resolver) or an _error_.

## Prod-write confirm

Every mutation on a tenant themed `prod` raises a centered modal:

```
You're writing to PROD — Are you sure?
  [y] yes   [n] cancel   [Esc] cancel
```

This is implemented as a single guard around the `AicClient` write methods
so it's automatic across all tabs — not per-call boilerplate.

## Reference apps (on the maintainer's machine)

- `~/w/tally` — multi-tab personal finance TUI. Closest overall structural
  match. Has the patterns to port (App state, InputMode dispatch, per-tab
  state isolation, `FilteredList<T>`, scroll-centered selection).
- `~/w/kbsr` — single-focus spaced-repetition TUI. Patterns to borrow for
  the env-picker modal centering and minimal/dim hint bar.
