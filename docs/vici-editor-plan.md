# Plan — vi editing in aic-edit's multiline fields

Adopt [`vici`](https://github.com/dbalmain/vici) (`~/w/headless-vi`) as the
editing engine behind `FieldKind::TextArea`, so every multiline field in the TUI
gets vi motions, operators, text objects and undo.

Status: **planned, not started.** Written 2026-08-02.

## 1. The surface

Every multiline editor in the app is the same widget — `TextField` with
`FieldKind::TextArea` (`src/tui/widgets/text_field.rs`). Nine call sites:

| Where                             | Sites                                           | Typical content              |
| --------------------------------- | ----------------------------------------------- | ---------------------------- |
| Managed field/object descriptions | `src/managed/state.rs:376, 658, 858, 918, 1116` | one line, occasionally three |
| ESV value editor                  | `src/esv/screen.rs:170, 196`                    | JSON, sometimes large        |
| ESV value viewer                  | `src/esv/view.rs:501`                           | read-only                    |
| Onboarding JWK paste              | `src/onboard/paste.rs` (`form.jwk_input`)       | one long pasted blob         |

That one widget is the whole leverage: this is a change to `src/tui/widgets/`,
not nine changes across seven feature directories. Nothing in the routing map
(CLAUDE.md §9) moves.

## 2. Scope decision: only `TextArea` gets a vici backend

`TextField` also serves `SingleLine` and `Masked`, which have `locked_prefix` —
a protected leading region (the `esv-` prefix) that vici has no concept of.
Rather than teach vici about protected regions, leave those two kinds on the
current implementation. `locked_prefix` is never used with `TextArea`, so the
problem disappears rather than needing a solution, and the blast radius halves.

Consequence: `TextField` grows a backend split. Keep the public surface
(`value`, `trimmed()`, `is_empty()`, `set()`, `handle_key`, `draw`) identical so
call sites don't change; `value` becomes a method rather than a field for the
textarea case, or is kept in sync. Prefer the former — see §3.1.

## 3. The three real obstacles

The obvious one (writing a vi mode) is the easy part. These are not.

### 3.1 Coordinates — one source of truth, not two

`TextField.cursor` is a **char index**; vici is **byte offsets** everywhere, by
deliberate design (it's what makes `Edit` convert into
`tree_sitter::InputEdit`). The renderer, `wrap_rows`, works in char ranges.

Commit `9e8941c` fixed a bug where the cursor column and the rendered wrap
disagreed about where a line ended. Holding a `String` + char cursor _alongside_
a vici `Editor` recreates exactly that class of defect, one layer up.

So: **do not mirror state.** The vici `Editor` becomes the sole owner of the
text and the caret. `TextField::value` becomes a derived read
(`ed.buffer().to_string()`, or better, borrow the rope), and `cursor`
disappears. Convert bytes → chars in exactly one place — inside
`wrap_rows`/`position_in_rows`, which already own the single wrap decision the
cursor and the render both derive from.

Cheapest robust move: change `wrap_rows` to work in **byte** ranges throughout,
matching vici, so the conversion count drops to zero. It already slices
`chars[row.start..row.end]`; slicing `&str` by byte range is strictly simpler.

### 3.2 `Esc` means two things — and the existing protocol already solves it

Every form uses `Esc` to cancel. vi uses it to leave insert mode.

`TextField::handle_key` already returns `bool` — "consumed, or fall through to
the form's dispatch". That is exactly the seam needed:

- `Esc` in insert mode → feed to vici, return `true`. The field goes to normal.
- `Esc` in normal mode → return `false`. The form cancels, as it does today.

No new plumbing, no changes to any feature's `keys.rs` binding table. A user who
never enters vi mode sees no change; a user in vi mode presses `Esc` twice to
cancel, which is the convention everywhere else vi lives inside a form.

### 3.3 `Tab` and `Enter` belong to the form, not the editor

vici binds `<Tab>` in insert mode to insert a tab byte, and `<CR>` to split the
row. In these forms `Tab`/`BackTab` move between fields, and `Enter` variously
saves, advances a field, or (in the ESV value editor and JWK paste) inserts a
newline via `TextField::push_newline`.

Resolve it in the keymap, not with an interception layer: `Editor::keymap_mut()`
is public and bindings are plain data, so unbind `<Tab>` from `Layer::Insert` on
construction. Then there is one description of who owns `Tab` — the form —
rather than a binding that exists and a host that shadows it.

`Enter` stays host-owned except at the two sites that already call
`push_newline`; those feed `<CR>` to vici instead.

`Ctrl-S` (save from any field, added in v0.4.0) is unbound in vi and needs no
handling.

## 4. Modal editing is opt-in, and off by default

Always-modal is wrong here. Four of the nine sites are `Description` fields
holding a single line; making the user press `i` before typing is a papercut on
every edit, forever, for no gain.

So the field starts in insert mode, behaving exactly as it does today, and vi
mode is reached by pressing `Esc`. That single rule gets it right for both
audiences: a user who doesn't know vi never presses `Esc` mid-field and never
notices; a user who does gets normal mode where their fingers expect it.

**Prerequisite gap:** there is no user-preferences store.
`src/vault/settings.rs` is auth-pending state, not preferences. If vi mode needs
to be switchable off entirely, that store has to exist first — see §8.

## 5. Divergences to accept knowingly

- **Visual rows vs buffer rows.** `draw_textarea` computes `scroll_y` in
  _wrapped visual rows_; vici's `Viewport.top_row` is a _buffer row_. With soft
  wrap, one buffer row spans several visual rows, so `<C-d>` and `H`/`M`/`L`
  will count buffer rows while the eye counts visual ones. vim has the same
  split (`j` vs `gj`). Accept it; don't try to reconcile.
- **Display width.** vici's sticky column counts graphemes; `wrap_rows` counts
  chars; neither counts terminal cells. Both are already wrong for CJK and tabs
  in the same direction, so this changes nothing — but it is now written down in
  two projects instead of one.
- **No registers across fields.** vici has one unnamed register per `Editor`, so
  yanking in one field and pasting in another won't work. Probably fine; if not,
  it argues for one shared `Editor` swapped between fields, which is worse.

## 6. Phases

**P0 — swap the engine, no vi.** `FieldKind::TextArea` is backed by a vici
`Editor`; existing key handling (arrows, Backspace, Ctrl-A/E, char insert) is
reimplemented as keys fed to vici in insert mode. Behaviour identical, tests
identical. This isolates the risky half — §3.1 — so that if the cursor drifts,
it is unambiguously the coordinate change and not the modal grammar.

Gate: the existing `wrap_rows` tests plus
`cursor_tracks_the_end_of_a_long_wrapped_word` must pass untouched.

**P1 — modal editing.** `Esc` enters normal mode; the `bool` protocol of §3.2
carries cancel. Footer hints show the mode. Everything vici 0.1 already has:
motions, operators, text objects, counts, visual mode, dot-repeat, macros, and
**undo** — which these fields have none of today.

**P2 — viewport.** Adopt vici 0.2 (shift + `Viewport` + `H`/`M`/`L`) and call
`set_viewport` from `draw_textarea`, which already computes both numbers. Gives
cursor-carrying `<C-d>`/`<C-f>` and screen motions. `>>`/`<<` land here too,
with `Indent` supplied from the app's own settings.

**P3 — search.** Once vici ships `/`, `?`, `n`, `N`. This is the phase that
makes the ESV value editor genuinely better rather than marginally different.

**P4 — optional.** `src/esv/view.rs:501` builds a `TextField` at render time, so
it holds no state and cannot host an editor as written. Moving it into state
would turn the read-only ESV viewer into a vi pager — motions and search over a
large value with no editing. Cheap once P3 exists, and arguably the best
value-per-line in the whole plan.

## 7. Dependency sequencing

vici **0.1.0** is published and has everything P0 and P1 need. Shift, viewport
and `H`/`M`/`L` are written but **uncommitted and unpublished** (see
`~/w/headless-vi/HANDOFF.md`); search and the jump list are not built.

So P0/P1 can start immediately against the published crate. P2 waits on a vici
0.2 release, P3 on 0.3. Use a `path` dependency only for local experiments —
`aic` is installed via `install.sh` and must build from a clean checkout.

Deps vici pulls in: `ropey` (new) and `unicode-segmentation` (already in
`Cargo.lock` at 1.13.2; vici wants ≥1.13.3, so a minor bump). Edition 2024 /
rust 1.85 match this crate exactly.

## 8. Honest assessment

The case for this is **weaker than it looks for modal editing and stronger than
it looks for undo.**

Five of nine sites are `Description` fields that usually hold one line — vi
motions buy almost nothing there. The JWK field is paste-only. The real prize is
the ESV value editor, and that is precisely the site that wants search, which
vici hasn't built.

But none of these fields has **undo at all** today. `TextField` has no history:
a mis-typed paste into a description or an ESV value is unrecoverable except by
retyping. vici brings `u`, `<C-r>` and `U` for free at P1, and that alone
probably justifies P0+P1 regardless of whether anyone ever presses `d`.

Recommendation: do P0 and P1, stop, and see whether P2/P3 are still wanted.

## 9. Open questions

1. **Does vi mode need an off switch?** If yes, a preferences store has to exist
   first (§4) — that's a real prerequisite, not a detail. If "`Esc` enters
   normal mode, always" is acceptable, the whole question disappears.
2. **Should `Enter` insert a newline, or save?** It currently does both,
   depending on the site. Worth unifying while touching this — but it's a
   behaviour change to fields people already use.
3. **P4 first?** The read-only viewer is the lowest-risk adoption (no editing,
   no `Esc` conflict, no form interaction) and would prove the rendering path
   before any editable field depends on it.
