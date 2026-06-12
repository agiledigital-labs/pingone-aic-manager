//! Single source of truth for key bindings.
//!
//! Dispatch, footer hints, and the F1 help popover all derive from the same
//! per-mode binding tables, so they can't drift — the class of bug where the
//! footer advertised `^S` but the handler ignored it in the secrets view.
//!
//! Rolled out per mode. Normal mode (the dashboard, where the drift bit) is
//! table-driven here; other modes are being migrated incrementally.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, InputMode, Realm, Tab};
use crate::esv::screen::Mode as EsvMode;
use crate::esv::state::{EditField, EsvView};
use crate::onboard::screen::Mode as OnboardMode;
use crate::secrets::screen::Mode as SecretsMode;

/// One key that fires a binding. Matching is by code + the ctrl modifier.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    Char(char),
    Ctrl(char),
    Code(KeyCode),
}

impl Trigger {
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
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Act {
    Quit,
    NextTab,
    PrevTab,
    ToggleView,
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
    Pull,
    Push,
    PullAll,
    Apply,
    Undo,
    UndoHistory,
    RealmToggle,
    TenantPicker,
    Onboard,
    AuthSettings,
    Lock,
}

/// A single binding: which keys fire it, how it's labelled in the footer / F1
/// help, and the action it runs.
pub struct Bind {
    pub triggers: &'static [Trigger],
    pub label: &'static str,
    pub desc: &'static str,
    pub footer: bool,
    pub help: bool,
    pub act: Act,
}

const fn b(
    triggers: &'static [Trigger],
    label: &'static str,
    desc: &'static str,
    footer: bool,
    help: bool,
    act: Act,
) -> Bind {
    Bind {
        triggers,
        label,
        desc,
        footer,
        help,
        act,
    }
}

/// The Normal-mode bindings for the current state. Conditional on tab, view,
/// and selection so the footer never advertises a key that won't fire.
pub fn normal_binds(app: &App) -> Vec<Bind> {
    use Act::*;
    let mut out: Vec<Bind> = Vec::new();

    // First-run / no tenants: only the bootstrap shortcuts make sense.
    if app.tenants.is_empty() {
        out.push(b(
            &[Trigger::Ctrl('t')],
            "^T",
            "add tenant",
            true,
            true,
            Onboard,
        ));
        out.push(b(
            &[Trigger::Ctrl('a')],
            "^A",
            "auth settings",
            true,
            true,
            AuthSettings,
        ));
        push_global(&mut out);
        return out;
    }

    let scripts_tab = app.current_tab == Tab::Scripts;
    let secrets = !scripts_tab && app.esv.view == EsvView::Secrets;
    let n = row_count(app);
    let can_apply = !scripts_tab
        && app
            .active_tenant()
            .map(|t| crate::esv::state::can_request_restart(app, &t.name))
            .unwrap_or(false);

    if can_apply {
        out.push(b(
            &[Trigger::Ctrl('s')],
            "^S",
            "apply changes",
            true,
            true,
            Apply,
        ));
    }
    // Tab / Shift-Tab cycle the top-level tabs (ESVs, Scripts, …). Only worth a
    // footer hint once there's more than one tab; help documents it regardless.
    let multi_tab = crate::app::Tab::all().len() > 1;
    out.push(b(
        &[Trigger::Code(KeyCode::Tab)],
        "Tab",
        "next tab",
        multi_tab,
        true,
        NextTab,
    ));
    out.push(b(
        &[Trigger::Code(KeyCode::BackTab)],
        "S-Tab",
        "prev tab",
        false,
        true,
        PrevTab,
    ));
    // `[` / `]` switch sub-tabs within a tab (here: Variables ⇄ Secrets) — the
    // lazygit/k9s convention for inner tabs.
    // `[` / `]` switch the ESV tab's inner sub-tabs (Variables ⇄ Secrets);
    // the Scripts tab has no sub-views.
    if !scripts_tab {
        out.push(b(
            &[Trigger::Char('['), Trigger::Char(']')],
            "[ ]",
            if secrets { "variables" } else { "secrets" },
            true,
            true,
            ToggleView,
        ));
    }
    out.push(b(&[Trigger::Char('/')], "/", "search", true, true, Search));

    // Movement (help-only; the footer stays uncluttered).
    out.push(b(
        &[Trigger::Char('j'), Trigger::Code(KeyCode::Down)],
        "j/↓",
        "move down",
        false,
        true,
        MoveDown,
    ));
    out.push(b(
        &[Trigger::Char('k'), Trigger::Code(KeyCode::Up)],
        "k/↑",
        "move up",
        false,
        true,
        MoveUp,
    ));
    out.push(b(&[Trigger::Char('g')], "g", "top", false, true, Top));
    out.push(b(&[Trigger::Char('G')], "G", "bottom", false, true, Bottom));
    out.push(b(
        &[Trigger::Code(KeyCode::PageDown)],
        "PgDn",
        "page down",
        false,
        true,
        PageDown,
    ));
    out.push(b(
        &[Trigger::Code(KeyCode::PageUp)],
        "PgUp",
        "page up",
        false,
        true,
        PageUp,
    ));

    if scripts_tab {
        if n > 0 {
            out.push(b(
                &[Trigger::Char('p'), Trigger::Code(KeyCode::Enter)],
                "p",
                "pull",
                true,
                true,
                Pull,
            ));
            out.push(b(&[Trigger::Char('P')], "P", "push", true, true, Push));
        }
        out.push(b(
            &[Trigger::Char('a')],
            "a",
            "pull all",
            true,
            true,
            PullAll,
        ));
    } else {
        if n > 0 {
            if secrets {
                out.push(b(
                    &[Trigger::Code(KeyCode::Enter), Trigger::Char('v')],
                    "Enter",
                    "versions",
                    true,
                    true,
                    Primary,
                ));
            } else {
                out.push(b(
                    &[Trigger::Code(KeyCode::Enter)],
                    "Enter",
                    "edit",
                    true,
                    true,
                    Primary,
                ));
            }
            out.push(b(
                &[Trigger::Char('d'), Trigger::Char('D')],
                "d",
                "delete",
                true,
                true,
                Delete,
            ));
        }

        out.push(b(
            &[Trigger::Ctrl('n')],
            "^N",
            if secrets {
                "new secret"
            } else {
                "new variable"
            },
            true,
            true,
            NewItem,
        ));
        out.push(b(&[Trigger::Ctrl('z')], "^Z", "undo", true, true, Undo));
        out.push(b(
            &[Trigger::Ctrl('y')],
            "^Y",
            "undo history",
            true,
            true,
            UndoHistory,
        ));
    }

    // Esc clears an active filter (only meaningful when one is applied).
    if filter_active(app) {
        out.push(b(
            &[Trigger::Code(KeyCode::Esc)],
            "Esc",
            "clear filter",
            false,
            true,
            ClearFilter,
        ));
    }

    // Global commands available on every populated screen. Realm toggle only
    // applies to the ESV tab — scripts are addressed by namespace, not realm.
    if !scripts_tab {
        out.push(b(
            &[Trigger::Char('r'), Trigger::Char('R')],
            "r",
            "switch realm",
            false,
            true,
            RealmToggle,
        ));
    }
    out.push(b(
        &[Trigger::Char('t'), Trigger::Char('T')],
        "t",
        "switch tenant",
        false,
        true,
        TenantPicker,
    ));
    out.push(b(
        &[Trigger::Ctrl('t')],
        "^T",
        "add tenant",
        false,
        true,
        Onboard,
    ));
    out.push(b(
        &[Trigger::Ctrl('a')],
        "^A",
        "auth settings",
        false,
        true,
        AuthSettings,
    ));
    out.push(b(
        &[Trigger::Char('L')],
        "L",
        "lock & quit",
        false,
        true,
        Lock,
    ));
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
        InputMode::Esv(EsvMode::Search) | InputMode::Scripts(_) => {
            vec![("Enter", "keep filter"), ("Esc", "clear + exit")]
        }
        InputMode::Secrets(SecretsMode::Create) => {
            use crate::secrets::state::CreateField;
            let mut out = vec![("Tab", "next field")];
            if let Some(form) = app.secret.create.as_ref() {
                match form.focused {
                    CreateField::Encoding | CreateField::Placeholders | CreateField::Json => {
                        out.push(("←/→", "change"))
                    }
                    CreateField::Value | CreateField::Save => out.push(("Enter", "create")),
                    _ => out.push(("Enter", "next")),
                }
            }
            out.push(("Esc", "cancel"));
            out
        }
        InputMode::Secrets(SecretsMode::Versions) => {
            use crate::secrets::state::DetailFocus;
            match app.secret.detail_focus {
                DetailFocus::Description => vec![
                    ("Tab", "versions"),
                    ("Enter", "save description"),
                    ("Esc", "close"),
                ],
                DetailFocus::Versions => vec![
                    ("Tab", "edit description"),
                    ("j/k", "navigate"),
                    ("e/d", "enable/disable"),
                    ("x", "destroy"),
                    ("^N", "add version"),
                    ("Esc", "close"),
                ],
            }
        }
        InputMode::Esv(EsvMode::Edit) => {
            let mut out = vec![("Tab", "navigate")];
            let focused = app.esv.editing.as_ref().map(|edit| edit.focused);
            match focused {
                Some(EditField::Id | EditField::Description | EditField::Type) => {
                    out.push(("Enter", "next"));
                }
                Some(EditField::Save) => out.push(("Enter", "save")),
                _ => {}
            }
            if focused == Some(EditField::Type) {
                out.push(("←/→", "change type"));
            }
            out.push(("Esc", "cancel"));
            out
        }
        InputMode::Vault(_) => Vec::new(),
        _ => Vec::new(),
    }
}

/// Quit bindings — present in every Normal state, never shown as hints.
fn push_global(out: &mut Vec<Bind>) {
    out.push(b(
        &[Trigger::Char('q')],
        "q",
        "quit",
        false,
        false,
        Act::Quit,
    ));
    out.push(b(
        &[Trigger::Ctrl('c')],
        "^C",
        "quit",
        false,
        false,
        Act::Quit,
    ));
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
        InputMode::ProdConfirm => crate::screens::prod_confirm::handle_key(app, key).await?,
        InputMode::UndoHistory => crate::screens::undo_history::handle_key(app, key),
        InputMode::Esv(mode) => crate::esv::screen::handle_key(app, key, mode)?,
        InputMode::Secrets(mode) => crate::secrets::screen::handle_key(app, key, mode)?,
        InputMode::Scripts(mode) => crate::scripts::screen::handle_key(app, key, mode),
    }
    Ok(())
}

/// Dispatch a Normal-mode key through the table. Returns without effect if no
/// binding matches.
pub async fn dispatch_normal(app: &mut App, key: KeyEvent) -> crate::Result<()> {
    let act = normal_binds(app)
        .iter()
        .find(|bind| bind.triggers.iter().any(|t| t.matches(&key)))
        .map(|bind| bind.act);
    if let Some(act) = act {
        run_normal(app, act).await;
    }
    Ok(())
}

async fn run_normal(app: &mut App, act: Act) {
    use Act::*;
    match act {
        Quit => app.should_quit = true,
        NextTab => cycle_tab(app, 1),
        PrevTab => cycle_tab(app, -1),
        ToggleView => app.esv.view = app.esv.view.toggled(),
        Search => {
            app.input_mode = if app.current_tab == Tab::Scripts {
                InputMode::Scripts(crate::scripts::screen::Mode::Search)
            } else {
                InputMode::Esv(EsvMode::Search)
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
        Pull => crate::scripts::screen::pull_selected(app),
        Push => crate::scripts::screen::push_selected(app),
        PullAll => crate::scripts::screen::pull_all(app),
        Apply => crate::esv::ops::request_restart(app),
        Undo => crate::esv::ops::request_latest_undo(app),
        UndoHistory => {
            app.undo_history_idx = 0;
            app.input_mode = InputMode::UndoHistory;
        }
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
    }
}

/// Cycle the top-level tab by `delta`, wrapping. No-op while only one tab
/// exists, but wired now so `Tab` behaves correctly as more tabs land.
fn cycle_tab(app: &mut App, delta: isize) {
    let tabs = crate::app::Tab::all();
    let n = tabs.len();
    if n < 2 {
        return;
    }
    let cur = tabs.iter().position(|t| *t == app.current_tab).unwrap_or(0) as isize;
    let next = (cur + delta).rem_euclid(n as isize) as usize;
    app.current_tab = tabs[next];
    // Lazily load the scripts list the first time the user lands on the tab —
    // it's a few list calls, so we don't pay for it unless it's visited.
    if app.current_tab == Tab::Scripts {
        crate::scripts::screen::refresh(app, false);
    }
}

fn row_count(app: &App) -> usize {
    if app.current_tab == Tab::Scripts {
        return app
            .scripts
            .matches(app.active_tenant().map(|t| t.name.as_str()))
            .len();
    }
    match app.esv.view {
        EsvView::Variables => app.esv_matches().len(),
        EsvView::Secrets => {
            crate::secrets::state::rows(app, app.active_tenant().map(|t| t.name.as_str())).len()
        }
    }
}

fn current_selection(app: &App) -> usize {
    if app.current_tab == Tab::Scripts {
        return app.scripts.selected;
    }
    match app.esv.view {
        EsvView::Variables => app.esv.list.selected,
        EsvView::Secrets => app.secret.list.selected,
    }
}

fn set_selection(app: &mut App, idx: usize) {
    let n = row_count(app);
    let clamped = if n == 0 { 0 } else { idx.min(n - 1) };
    if app.current_tab == Tab::Scripts {
        app.scripts.selected = clamped;
        return;
    }
    match app.esv.view {
        EsvView::Variables => app.esv.list.selected = clamped,
        EsvView::Secrets => app.secret.list.selected = clamped,
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
    if app.current_tab == Tab::Scripts {
        return !app.scripts.query.is_empty();
    }
    match app.esv.view {
        EsvView::Variables => !app.esv.list.query.is_empty(),
        EsvView::Secrets => !app.secret.list.query.is_empty(),
    }
}

fn clear_filter(app: &mut App) {
    if app.current_tab == Tab::Scripts {
        app.scripts.reset_view();
        return;
    }
    match app.esv.view {
        EsvView::Variables => app.esv.reset_view(),
        EsvView::Secrets => {
            app.secret.list.query.clear();
            app.secret.list.selected = 0;
            app.secret.list.scroll = 0;
        }
    }
}

fn primary(app: &mut App) {
    match app.esv.view {
        EsvView::Variables => crate::esv::screen::start_edit(app),
        EsvView::Secrets => crate::secrets::screen::open_versions(app),
    }
}

fn delete(app: &mut App) {
    match app.esv.view {
        EsvView::Variables => crate::esv::ops::request_delete(app),
        EsvView::Secrets => crate::secrets::screen::request_delete(app),
    }
}

fn new_item(app: &mut App) {
    match app.esv.view {
        EsvView::Variables => crate::esv::screen::start_create(app),
        EsvView::Secrets => crate::secrets::screen::start_create(app),
    }
}
