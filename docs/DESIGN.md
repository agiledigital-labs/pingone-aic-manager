# pingone-aic-manager — TUI design rules

These are the agreed visual + interaction rules for `pingone-aic-manager`. Apply
them whenever building or reviewing UI code. They were arrived at by reviewing
two existing Ratatui apps the maintainer wrote (`tally`, `kbsr`) and a short
design conversation that pinned down env chrome, issue surfacing, and
keybindings. Don't redebate these unless the maintainer explicitly revisits.

## Borders, separators, palette

- **No borders on main panels.** Borders only on confirmation/error modals.
- Use whitespace, position, and color for hierarchy.
- Transparent backgrounds. Selection = `bg(DarkGray)` on the row (no `>` glyph).
- Labels in `DarkGray`. Values in `White` (or semantic color).
- Semantic colors: red=out/error, green=in/ok, yellow=action/caution,
  cyan=highlight/key-hints, blue=informational, magenta=rare-special.

## Navigation

- The header shows the active function in bold white; there is no tab strip.
- `Ctrl-P` opens the function selector in fuzzy-search mode. Typing filters
  immediately; arrows navigate, Enter opens, and Esc cancels.

## Terminal size

- **80 columns is the supported minimum width.** Everything must remain legible
  and unambiguous at 80; below that, degradation is allowed to be ugly. Layout
  that only works wider than 80 is a defect, and a test that only passes wider
  than 80 is testing the wrong width.
- Consequences worth knowing before you lay out a table:
  - Prefer `Percentage` constraints summing to exactly 100 over a mix that
    includes `Length`. Ratatui's solver satisfies `Length` first, so an
    over-subscribed mixed set starves the percentage columns to nothing and
    clips them **without an ellipsis**. All-percentage degrades proportionally.
    This is about **table column** sets, where a starved column silently loses
    content. A small fixed gutter between panes is fine — `access/view.rs`'s
    `BODY_COLUMNS` is `[Percentage(62), Length(2), Percentage(38)]`, over-
    subscribed by those two columns on purpose.
  - Truncate through `tui::list_chrome::truncate_metadata` so a clipped value
    says so. A cell built as a styled `Line` bypasses that helper and gets
    ratatui's silent clip instead — if a column is styled per-glyph, check its
    header and its content both still fit at 80.
  - `tui::modal_chrome::CONTENT_WIDTH` is also 80, so at the minimum width a
    modal is exactly as wide as the screen. That is intended: 80 is the design
    width, and wider terminals get margin rather than a wider modal.
- Height has no stated minimum, and the two kinds of overflow behave
  differently:
  - **Detail panes scroll** (`tui::list_chrome::DetailScroll`), which is the
    general answer to a short viewport. Use `clamp_wrapping` when the pane lets
    the widget wrap and `clamp` when it has pre-wrapped its own rows — a height
    measured the wrong way gives a too-small limit, and a pane that stops short
    of its content looks exactly like one that reached the end.
  - **Modals are clipped, not scrolled.** `modal_chrome` sizes a modal to its
    content and then clamps to the screen, so the last rows simply vanish on a
    short terminal. Keep a modal's committing control and its error line
    **above** its optional rows, or the two things the operator needs on a
    cramped screen are the two that disappear.

## Within-view layout

- **Yazi-style miller** for rich content views (Scripts, OAuth2, SAML): list on
  the left (~30%), detail/editor on the right. Always visible. Enter focuses the
  right pane.
- For simple list+short-detail views (ESVs), tally's list-top +
  fixed-detail-strip pattern is fine.

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
  most-commonly-added. Extra environments (e.g. predev, UAT) pick one of the
  four themes.

## Issue surfacing

- **Inline strip** at the top of the relevant pane for actionable state (e.g.
  yellow strip "⚠ restart pending — 3 ESVs changed [a] apply" in the ESV pane;
  red strip "push conflict — remote drifted" in the Scripts pane).
- **Top-right toast overlay** for transient events (token refreshed, push
  succeeded, fetched 47 scripts). Auto-dismiss; stack vertically.

## Keybindings

- Vim-like movement where it fits: `j`/`k`, arrows, `gg`/`G`. `/` for search.
- `Esc` cancels or closes the active mode. `Enter` confirms, advances, focuses,
  or edits depending on focus.
- Prefer plain keys for local actions. Ctrl combos are acceptable for global
  commands or where plain keys conflict with text entry.
- Mouse capture on but not relied on (mirrors tally).

## Key hint principles

- Hints describe what will happen **now**, not what a key usually does.
- Use exact verbs: `next`, `save`, `edit`, `cancel`, `apply`.
- Do not show a key if the focused control captures it for another purpose.
  Example: hide `Enter` while an ESV value textarea uses it for newlines.
- The bottom bar is for immediate, high-value actions only.
- General movement and global commands belong in the keybind popover, not every
  footer.
- When a modal is open, the modal owns all visible hints.
- The complete keybind popover must list every active key for the current mode.
- Hint rendering and key dispatch should come from the same action source.

## Modals

- Used sparingly: env picker, confirm-destructive, push-conflict resolver, error
  reporter. Rendered with `Clear` + black bg; border + title only when the modal
  is a _choice_ (env picker, conflict resolver) or an _error_.

## Prod-write confirm

Every mutation on a tenant themed `prod` raises a centered modal:

```
You're writing to PROD — Are you sure?
  [y] yes   [n] cancel   [Esc] cancel
```

This is implemented as a single guard around the `AicClient` write methods so
it's automatic across all views — not per-call boilerplate.

## Reference apps (on the maintainer's machine)

- `~/w/tally` — multi-tab personal finance TUI. Closest overall structural
  match. Has the patterns to port (App state, InputMode dispatch, per-tab state
  isolation, `FilteredList<T>`, scroll-centered selection).
- `~/w/kbsr` — single-focus spaced-repetition TUI. Patterns to borrow for the
  env-picker modal centering and minimal/dim hint bar.
