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

/// The Normal-mode bindings for the current state. Conditional on active view,
/// and selection so the footer never advertises a key that won't fire.
pub fn normal_binds(app: &App) -> Vec<Bind> {
    use Act::*;
    let mut out: Vec<Bind> = Vec::new();

    out.push(b(
        &[Trigger::Ctrl('p')],
        "Ctrl-P",
        "functions",
        true,
        true,
        Functions,
    ));

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

    let scripts_view = app.active_view == View::Scripts;
    let esv_view_active = app.active_view == View::Esvs;
    let managed_view = app.active_view == View::Managed;
    let mappings_view = app.active_view == View::Mappings;
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
        out.push(b(
            &[Trigger::Ctrl('s')],
            "^S",
            "apply changes",
            true,
            true,
            Apply,
        ));
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
        out.push(b(
            &[Trigger::Char('[')],
            "[",
            prev_label,
            true,
            true,
            PrevView,
        ));
        out.push(b(
            &[Trigger::Char(']')],
            "]",
            next_label,
            true,
            true,
            NextView,
        ));
    }
    out.push(b(&[Trigger::Char('/')], "/", "search", true, true, Search));

    // Movement (help-only; the footer stays uncluttered).
    out.push(b(
        &[Trigger::Char('j'), Trigger::Code(KeyCode::Down)],
        "↓",
        "move down",
        false,
        true,
        MoveDown,
    ));
    out.push(b(
        &[Trigger::Char('k'), Trigger::Code(KeyCode::Up)],
        "↑",
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

    if scripts_view {
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
    } else if esv_view_active {
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
            } else if mappings {
                out.push(b(
                    &[Trigger::Char('e')],
                    "e",
                    "edit alias",
                    true,
                    true,
                    Primary,
                ));
                out.push(b(
                    &[Trigger::Char('d'), Trigger::Char('D')],
                    "d",
                    "remove",
                    true,
                    true,
                    Delete,
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
            if !mappings {
                out.push(b(
                    &[Trigger::Char('d'), Trigger::Char('D')],
                    "d",
                    "delete",
                    true,
                    true,
                    Delete,
                ));
            }
        }

        if !mappings {
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
        } else {
            out.push(b(&[Trigger::Char('a')], "a", "add", true, true, NewItem));
        }
        out.push(b(&[Trigger::Ctrl('z')], "^Z", "undo", true, true, Undo));
        out.push(b(
            &[Trigger::Ctrl('y')],
            "^Y",
            "undo history",
            true,
            true,
            UndoHistory,
        ));
    } else if managed_view && n > 0 {
        out.push(b(
            &[Trigger::Code(KeyCode::Enter)],
            "Enter",
            "edit field",
            true,
            true,
            Primary,
        ));
        out.push(b(&[Trigger::Char('a')], "a", "add", true, true, NewItem));
        out.push(b(
            &[Trigger::Char('r')],
            "r",
            "rename field",
            true,
            true,
            RenameField,
        ));
        out.push(b(
            &[Trigger::Char('R')],
            "R",
            "rename object",
            true,
            true,
            RenameObject,
        ));
        out.push(b(
            &[Trigger::Char('h')],
            "h",
            "add hook",
            true,
            true,
            AddHook,
        ));
        out.push(b(
            &[Trigger::Char('d'), Trigger::Char('D')],
            "d",
            "delete field",
            true,
            true,
            Delete,
        ));
        out.push(b(
            &[Trigger::Char('[')],
            "[",
            "previous field",
            false,
            true,
            PrevField,
        ));
        out.push(b(
            &[Trigger::Char(']')],
            "]",
            "next field",
            false,
            true,
            NextField,
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
    } else if mappings_view && n > 0 {
        out.push(b(
            &[Trigger::Char('r')],
            "r",
            "reconcile",
            true,
            true,
            ReconMapping,
        ));
        out.push(b(
            &[Trigger::Char('p')],
            "p",
            "pull scripts",
            true,
            true,
            PullMappingScripts,
        ));
    } else if oauth_view && n > 0 {
        out.push(b(
            &[Trigger::Code(KeyCode::Enter)],
            "Enter",
            "inspect",
            true,
            true,
            Primary,
        ));
        out.push(b(
            &[Trigger::Ctrl('d')],
            "^D",
            "scroll detail down",
            true,
            true,
            DetailScrollDown,
        ));
        out.push(b(
            &[Trigger::Ctrl('u')],
            "^U",
            "scroll detail up",
            true,
            true,
            DetailScrollUp,
        ));
    }
    if managed_view {
        out.push(b(
            &[Trigger::Ctrl('n')],
            "^N",
            "new object",
            true,
            true,
            NewObject,
        ));
    }
    if managed_view || mappings_view || idmstore_view || oauth_view || mappings {
        out.push(b(
            &[Trigger::Ctrl('r')],
            "^R",
            "refresh",
            true,
            true,
            Refresh,
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
    // applies to the ESV view — scripts are addressed by namespace, not realm.
    if esv_view_active {
        let realm_triggers: &'static [Trigger] = if mappings {
            &[Trigger::Char('r')]
        } else {
            &[Trigger::Char('r'), Trigger::Char('R')]
        };
        out.push(b(
            realm_triggers,
            if mappings { "r" } else { "r/R" },
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
        InputMode::Managed(_) => crate::managed::screen::footer_hints(app),
        InputMode::Mappings(_) => crate::mappings::screen::footer_hints(app),
        InputMode::IdmStore(_) => crate::idmstore::screen::footer_hints(app),
        InputMode::Oauth(mode) => {
            let mut out = crate::oauth::screen::footer_hints(app);
            if mode == crate::oauth::screen::Mode::Normal {
                out.insert(0, ("Ctrl-P", "functions"));
            }
            out
        }
        InputMode::Selector => Vec::new(),
        InputMode::Secretmap(_) => crate::secretmap::screen::footer_hints(app),
        InputMode::Secrets(SecretsMode::Create) => {
            let mut out = vec![("Tab", "next field")];
            if let Some(focused) = crate::secrets::screen::create_focus(app) {
                match focused {
                    crate::secrets::state::CreateField::Encoding
                    | crate::secrets::state::CreateField::Placeholders
                    | crate::secrets::state::CreateField::Json => out.push(("←/→", "change")),
                    crate::secrets::state::CreateField::Value
                    | crate::secrets::state::CreateField::Save => out.push(("Enter", "create")),
                    _ => out.push(("Enter", "next")),
                }
            }
            out.push(("Esc", "cancel"));
            out
        }
        InputMode::Secrets(SecretsMode::Versions) => {
            match crate::secrets::screen::detail_focus(app) {
                crate::secrets::state::DetailFocus::Description => vec![
                    ("Tab", "versions"),
                    ("Enter", "save description"),
                    ("Esc", "close"),
                ],
                crate::secrets::state::DetailFocus::Versions => vec![
                    ("Tab", "edit description"),
                    ("↑/↓", "navigate"),
                    ("e/d", "enable/disable"),
                    ("x", "destroy"),
                    ("^N", "add version"),
                    ("Esc", "close"),
                ],
            }
        }
        InputMode::Esv(EsvMode::Edit) => {
            let mut out = vec![("Tab", "navigate")];
            let focused = crate::esv::screen::edit_focused(app);
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
        InputMode::Selector => crate::app::selector::handle_key(app, key),
        InputMode::ProdConfirm => crate::app::prod_confirm::handle_key(app, key).await?,
        InputMode::UndoHistory => crate::undo::screen::handle_key(app, key),
        InputMode::Esv(mode) => crate::esv::screen::handle_key(app, key, mode)?,
        InputMode::Secrets(mode) => crate::secrets::screen::handle_key(app, key, mode)?,
        InputMode::Scripts(mode) => crate::scripts::screen::handle_key(app, key, mode),
        InputMode::Managed(mode) => crate::managed::screen::handle_key(app, key, mode),
        InputMode::Mappings(mode) => crate::mappings::screen::handle_key(app, key, mode),
        InputMode::IdmStore(mode) => crate::idmstore::screen::handle_key(app, key, mode),
        InputMode::Oauth(mode) => crate::oauth::screen::handle_key(app, key, mode),
        InputMode::Secretmap(mode) => crate::secretmap::screen::handle_key(app, key, mode),
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
            if app.active_view == View::Managed {
                crate::managed::ops::request_latest_undo(app);
            } else if app.active_view == View::Esvs
                && crate::esv::screen::current_view(app) == EsvView::Mappings
            {
                crate::secretmap::ops::request_latest_undo(app);
            } else {
                crate::esv::ops::request_latest_undo(app);
            }
        }
        UndoHistory => {
            app.undo_history_idx = 0;
            app.input_mode = InputMode::UndoHistory;
        }
        PrevField => crate::managed::screen::move_property(app, -1),
        NextField => crate::managed::screen::move_property(app, 1),
        DetailScrollDown => crate::oauth::screen::scroll_detail(app, 10),
        DetailScrollUp => crate::oauth::screen::scroll_detail(app, -10),
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
        View::IdmStore => InputMode::IdmStore(crate::idmstore::screen::Mode::Search),
        View::Oauth => InputMode::Oauth(crate::oauth::screen::Mode::Search),
        View::Esvs => InputMode::Esv(EsvMode::Search),
    }
}
