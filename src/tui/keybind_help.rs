//! Confirm-style keybind help popover.
//!
//! This is intentionally a small overlay rather than a new screen. The
//! underlying input mode stays active, but while the popover is open the app
//! dispatches only the popover's close keys.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Padding, Paragraph, Wrap},
};

use crate::app::{App, InputMode};
use crate::esv::screen::Mode as EsvMode;
use crate::esv::state::EditField;
use crate::managed::screen::Mode as ManagedMode;
use crate::oauth::screen::Mode as OauthMode;
use crate::onboard::screen::Mode as OnboardMode;
use crate::secrets::screen::Mode as SecretsMode;
use crate::tui::modal_chrome::hint_line;
use crate::vault::screen::Mode as VaultMode;
use crate::vault::setup::{AuthMethod, SetupContext};

const WIDTH: u16 = 84;

pub fn draw(f: &mut Frame, app: &App) {
    let lines = lines_for(app);
    let area = f.area();
    let max_height = area.height.saturating_sub(2).max(1);
    let desired_height = (lines.len() as u16).saturating_add(5).max(10);
    let popup = centered(area, WIDTH, desired_height.min(max_height));

    f.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .padding(Padding::new(2, 2, 1, 1))
        .title(Span::styled(
            " Keybinds ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let chunks = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(inner);

    f.render_widget(
        Paragraph::new(lines)
            .style(Style::default().fg(Color::White))
            .wrap(Wrap { trim: false }),
        chunks[0],
    );
    f.render_widget(
        Paragraph::new(hint_line(&[("Esc/Enter", "close"), ("F1/?", "close")])),
        chunks[2],
    );
}

fn lines_for(app: &App) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    match app.input_mode {
        InputMode::Normal => normal_lines(app, &mut lines),
        InputMode::Esv(EsvMode::Search) | InputMode::Scripts(_) => esv_search_lines(&mut lines),
        InputMode::Managed(mode) => managed_lines(mode, &mut lines),
        InputMode::Oauth(mode) => oauth_lines(mode, &mut lines),
        InputMode::Esv(EsvMode::Edit) => esv_edit_lines(app, &mut lines),
        InputMode::Esv(EsvMode::RestartConfirm) => confirm_lines(
            &mut lines,
            "Apply pending changes",
            &[("y", "restart tenant runtime"), ("n/Esc", "cancel")],
        ),
        InputMode::Esv(EsvMode::DeleteConfirm) => confirm_lines(
            &mut lines,
            "Delete ESV variable",
            &[("y", "delete variable"), ("n/Esc", "cancel")],
        ),
        InputMode::Vault(VaultMode::Settings) => auth_settings_lines(app, &mut lines),
        InputMode::Vault(VaultMode::SettingsConfirm) => confirm_lines(
            &mut lines,
            "Auth Settings confirmation",
            &[("y", "confirm"), ("n/Esc", "cancel")],
        ),
        InputMode::Vault(VaultMode::SettingsRename) => text_modal_lines(
            &mut lines,
            "Rename security key",
            &[("Enter", "save"), ("Esc", "cancel")],
        ),
        InputMode::Vault(VaultMode::Setup) => setup_auth_lines(app, &mut lines),
        InputMode::Vault(VaultMode::Unlock) => unlock_lines(app, &mut lines),
        InputMode::Onboard(mode) => onboard_lines(app, mode, &mut lines),
        InputMode::EnvPicker => env_picker_lines(app, &mut lines),
        InputMode::ProdConfirm => confirm_lines(
            &mut lines,
            "Production write confirmation",
            &[("y", "confirm production write"), ("n/Esc", "cancel")],
        ),
        InputMode::UndoHistory => undo_history_lines(&mut lines),
        InputMode::Secrets(SecretsMode::Create) => text_modal_lines(
            &mut lines,
            "New secret",
            &[
                ("Tab/Shift-Tab", "move between fields"),
                ("←/→ or Space", "encoding / toggles"),
                ("Enter", "create"),
                ("Esc", "cancel"),
            ],
        ),
        InputMode::Secrets(SecretsMode::Versions) => text_modal_lines(
            &mut lines,
            "Secret versions",
            &[
                ("Tab", "edit description / versions"),
                ("j/k", "navigate"),
                ("e/d", "enable / disable"),
                ("x/Del", "destroy (irreversible)"),
                ("^N", "add version"),
                ("Esc", "close"),
            ],
        ),
        InputMode::Secrets(SecretsMode::AddVersion) => text_modal_lines(
            &mut lines,
            "Add secret version",
            &[("Enter", "add version"), ("Esc", "cancel")],
        ),
        InputMode::Secrets(SecretsMode::DeleteConfirm) => confirm_lines(
            &mut lines,
            "Delete secret",
            &[("y", "delete secret + all versions"), ("n/Esc", "cancel")],
        ),
        InputMode::Secrets(SecretsMode::VersionDestroyConfirm) => confirm_lines(
            &mut lines,
            "Destroy secret version",
            &[("y", "destroy version (irreversible)"), ("n/Esc", "cancel")],
        ),
    }
    lines
}

fn oauth_lines(mode: OauthMode, lines: &mut Vec<Line<'static>>) {
    match mode {
        OauthMode::Search => esv_search_lines(lines),
        OauthMode::Normal => text_modal_lines(
            lines,
            "OAuth client config",
            &[
                ("j/k or ↑/↓", "move selection"),
                ("Enter", "load selected client"),
                ("^U/^D", "scroll detail"),
                ("R", "refresh"),
                ("Esc", "back"),
            ],
        ),
    }
}

fn normal_lines(app: &App, lines: &mut Vec<Line<'static>>) {
    // Derived from the same keymap table that drives dispatch + footer, so the
    // help can't list a key the dispatcher won't honour (or omit one it does).
    group(lines, "Keys");
    for binding in crate::app::keymap::normal_binds(app) {
        if binding.help {
            bind(lines, binding.label, binding.desc);
        }
    }
    bind(lines, "q / ^C", "quit");
    bind(lines, "F1/?", "show keybinds");
}

fn esv_search_lines(lines: &mut Vec<Line<'static>>) {
    group(lines, "Search");
    bind(lines, "Type", "edit search query");
    bind(lines, "Backspace", "delete character");
    bind(lines, "Enter", "keep filter and return to list");
    bind(lines, "Esc", "clear filter and return to list");
    group(lines, "Results");
    bind(lines, "↑/↓", "move selection");
    bind(lines, "PgUp/PgDn", "move by page");
    bind(lines, "F1", "show keybinds");
}

fn esv_edit_lines(app: &App, lines: &mut Vec<Line<'static>>) {
    group(lines, "Edit variable");
    bind(lines, "Tab/Shift-Tab", "move between fields");
    match app.esv.editing.as_ref().map(|edit| edit.focused) {
        Some(EditField::Id | EditField::Description | EditField::Type) => {
            bind(lines, "Enter", "move to next field");
        }
        Some(EditField::Value) => {
            bind(lines, "Enter", "insert newline in value");
        }
        Some(EditField::Save) => {
            bind(lines, "Enter", "save variable");
        }
        None => {}
    }
    bind(lines, "Esc", "cancel edit");
    group(lines, "Selector");
    bind(
        lines,
        "←/→",
        "change type when the Type selector is focused",
    );
    group(lines, "Text fields");
    bind(lines, "Arrows/Home/End", "move cursor");
    bind(lines, "Backspace/Delete", "delete text");
    bind(lines, "F1", "show keybinds");
}

fn managed_lines(mode: ManagedMode, lines: &mut Vec<Line<'static>>) {
    match mode {
        ManagedMode::Search => esv_search_lines(lines),
        ManagedMode::EditField => text_modal_lines(
            lines,
            "Edit managed field",
            &[
                ("Tab/Shift-Tab", "move between fields"),
                ("Enter", "advance, toggle, or save"),
                ("Space", "toggle focused checkbox"),
                ("Esc", "cancel"),
            ],
        ),
        ManagedMode::AddField => text_modal_lines(
            lines,
            "Add managed field",
            &[
                ("Tab/Shift-Tab", "move between fields"),
                ("←/→ or Space", "change type or toggles"),
                ("Enter", "advance or add"),
                ("Esc", "cancel"),
            ],
        ),
        ManagedMode::AddRelationship => text_modal_lines(
            lines,
            "Add relationship",
            &[
                ("Tab/Shift-Tab", "move between fields"),
                ("Enter", "advance, pick target, or add"),
                ("Space", "toggle focused checkbox"),
                ("Esc", "cancel"),
            ],
        ),
        ManagedMode::PickRelationshipTarget => text_modal_lines(
            lines,
            "Pick target object",
            &[
                ("Type", "filter targets"),
                ("j/k or ↑/↓", "move selection"),
                ("Enter", "choose target"),
                ("Esc", "back"),
            ],
        ),
        ManagedMode::AddHook => text_modal_lines(
            lines,
            "Register hook",
            &[
                ("j/k or ↑/↓", "move selection"),
                ("Enter", "register selected hook"),
                ("Esc", "cancel"),
            ],
        ),
        ManagedMode::DeleteFieldConfirm => confirm_lines(
            lines,
            "Delete managed field",
            &[("y", "delete field"), ("n/Esc", "cancel")],
        ),
    }
}

fn auth_settings_lines(app: &App, lines: &mut Vec<Line<'static>>) {
    group(lines, "Auth Settings");
    bind(lines, "p", "set or change password");
    bind(lines, "s", "add security key");
    if !app.wraps.wraps.is_empty() {
        bind(lines, "d", "remove selected factor");
        bind(lines, "Enter", "edit selected factor");
    }
    bind(lines, "Esc", "close");
    group(lines, "Movement");
    bind(lines, "j/k or ↑/↓", "move selection");
    if let Some(range) = number_range(app.wraps.wraps.len()) {
        bind(lines, range, "edit numbered factor");
    }
    bind(lines, "F1/?", "show keybinds");
}

fn setup_auth_lines(app: &App, lines: &mut Vec<Line<'static>>) {
    if app.auth_setup.form.busy {
        group(lines, "Security key enrollment");
        bind(lines, "F1", "show or close keybinds");
        bind(
            lines,
            "Input",
            "temporarily locked while waiting for the security key",
        );
        return;
    }

    let first_run = app.auth_setup.context == SetupContext::FirstRun;
    group(
        lines,
        if first_run {
            "Set up authentication"
        } else {
            "Add factor"
        },
    );
    bind(lines, "Enter", "advance or submit");
    bind(lines, "Esc", if first_run { "quit" } else { "cancel" });
    bind(lines, "Tab/Shift-Tab", "move between fields");
    if first_run {
        bind(lines, "←/→ or Space", "change authentication method");
    }
    match app.auth_setup.form.method {
        AuthMethod::None => {}
        AuthMethod::Password => {
            bind(lines, "Type", "enter password");
            bind(lines, "Backspace", "delete character");
        }
        AuthMethod::SecurityKey => {
            bind(lines, "Type", "enter PIN or label");
            bind(lines, "Backspace", "delete character");
        }
    }
    bind(lines, "F1", "show keybinds");
}

fn unlock_lines(app: &App, lines: &mut Vec<Line<'static>>) {
    group(lines, "Unlock");
    bind(lines, "Enter", "submit focused credential");
    bind(lines, "Esc", "quit");
    if app.wraps.has_password() && app.wraps.has_security_key() {
        bind(lines, "Tab/Shift-Tab", "switch unlock method");
    }
    bind(lines, "Type", "enter password or security-key PIN");
    bind(lines, "F1", "show keybinds");
}

fn onboard_menu_lines(app: &App, lines: &mut Vec<Line<'static>>) {
    group(lines, "Add Tenant");
    bind(lines, "Enter", "choose selected method");
    bind(lines, "Esc", "cancel");
    group(lines, "Movement");
    bind(lines, "j/k or ↑/↓", "move selection");
    let count = if app.has_env_creds { 4 } else { 3 };
    if let Some(range) = number_range(count) {
        bind(lines, range, "choose numbered method");
    }
    bind(lines, "F1/?", "show keybinds");
}

fn onboard_lines(app: &App, mode: OnboardMode, lines: &mut Vec<Line<'static>>) {
    match mode {
        OnboardMode::Menu => onboard_menu_lines(app, lines),
        OnboardMode::Cookie | OnboardMode::Paste => onboard_form_lines(lines),
        OnboardMode::UserPass => onboard_userpass_lines(app, lines),
        OnboardMode::OverwriteConfirm => confirm_lines(
            lines,
            "Overwrite existing tenant",
            &[("y", "overwrite"), ("n/Esc", "cancel")],
        ),
    }
}

fn onboard_form_lines(lines: &mut Vec<Line<'static>>) {
    group(lines, "Add Tenant form");
    bind(lines, "Enter", "advance or submit");
    bind(lines, "Esc", "go back");
    bind(lines, "Tab/Shift-Tab", "move between fields");
    bind(
        lines,
        "←/→",
        "change theme when the Theme selector is focused",
    );
    group(lines, "Text fields");
    bind(lines, "Arrows/Home/End", "move cursor");
    bind(lines, "Backspace/Delete", "delete text");
    bind(lines, "F1", "show keybinds");
}

fn onboard_userpass_lines(app: &App, lines: &mut Vec<Line<'static>>) {
    if app
        .onboard
        .up_form
        .as_ref()
        .is_some_and(|form| form.pending_prompt.is_some())
    {
        group(lines, "Additional input");
        bind(lines, "Type", "enter requested code");
        bind(lines, "Backspace", "delete character");
        bind(lines, "Enter", "submit code");
        bind(lines, "Esc", "cancel prompt");
        bind(lines, "F1", "show keybinds");
    } else {
        onboard_form_lines(lines);
    }
}

fn env_picker_lines(app: &App, lines: &mut Vec<Line<'static>>) {
    group(lines, "Switch Tenant");
    bind(lines, "Enter", "switch to selected tenant");
    bind(lines, "Esc", "cancel");
    group(lines, "Movement");
    bind(lines, "j/k or ↑/↓", "move selection");
    if let Some(range) = number_range(app.tenants.len()) {
        bind(lines, range, "switch to numbered tenant");
    }
    bind(lines, "F1/?", "show keybinds");
}

fn undo_history_lines(lines: &mut Vec<Line<'static>>) {
    group(lines, "Undo History");
    bind(lines, "Enter", "undo selected pending entry");
    bind(lines, "Esc", "close");
    group(lines, "Movement");
    bind(lines, "j/k or ↑/↓", "move selection");
    bind(lines, "F1", "show keybinds");
}

fn confirm_lines(
    lines: &mut Vec<Line<'static>>,
    title: &'static str,
    bindings: &[(&'static str, &'static str)],
) {
    group(lines, title);
    for (key, desc) in bindings {
        bind(lines, *key, *desc);
    }
}

fn text_modal_lines(
    lines: &mut Vec<Line<'static>>,
    title: &'static str,
    bindings: &[(&'static str, &'static str)],
) {
    confirm_lines(lines, title, bindings);
    group(lines, "Text field");
    bind(lines, "Arrows/Home/End", "move cursor");
    bind(lines, "Backspace/Delete", "delete text");
    bind(lines, "Type", "edit value");
    bind(lines, "F1", "show keybinds");
}

fn group(lines: &mut Vec<Line<'static>>, title: &'static str) {
    if !lines.is_empty() {
        lines.push(Line::from(""));
    }
    lines.push(Line::from(Span::styled(
        title,
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )));
}

fn bind(lines: &mut Vec<Line<'static>>, key: impl Into<String>, description: impl Into<String>) {
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            format!("{:<18}", key.into()),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(description.into()),
    ]));
}

fn number_range(count: usize) -> Option<String> {
    let max = count.min(9);
    match max {
        0 => None,
        1 => Some("1".to_string()),
        _ => Some(format!("1-{max}")),
    }
}

fn centered(parent: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(parent.width);
    let h = height.min(parent.height);
    Rect {
        x: parent.x + (parent.width.saturating_sub(w)) / 2,
        y: parent.y + (parent.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    }
}
