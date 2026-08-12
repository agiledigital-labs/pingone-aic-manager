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
        InputMode::Esv(mode) => esv_lines(app, mode, &mut lines),
        InputMode::Scripts(mode) => {
            feature_lines(crate::scripts::screen::help_lines(mode), &mut lines)
        }
        InputMode::Managed(mode) => {
            feature_lines(crate::managed::screen::help_lines(mode, app), &mut lines)
        }
        InputMode::Mappings(mode) => {
            feature_lines(crate::mappings::screen::help_lines(mode), &mut lines)
        }
        InputMode::Access(mode) => {
            feature_lines(crate::access::screen::help_lines(mode), &mut lines)
        }
        InputMode::IdmStore(mode) => {
            feature_lines(crate::idmstore::screen::help_lines(mode), &mut lines)
        }
        InputMode::Oauth(mode) => feature_lines(crate::oauth::screen::help_lines(mode), &mut lines),
        InputMode::Secretmap(mode) => {
            feature_lines(crate::secretmap::screen::help_lines(mode), &mut lines)
        }
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
        InputMode::Vault(VaultMode::Unlock | VaultMode::Relock) => unlock_lines(app, &mut lines),
        InputMode::Onboard(mode) => onboard_lines(app, mode, &mut lines),
        InputMode::EnvPicker => env_picker_lines(app, &mut lines),
        InputMode::Selector => selector_lines(&mut lines),
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
                ("↑/↓", "navigate"),
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

fn normal_lines(app: &App, lines: &mut Vec<Line<'static>>) {
    // Derived from the same keymap table that drives dispatch + footer, so the
    // help can't list a key the dispatcher won't honour (or omit one it does).
    group(lines, "Keys");
    for binding in crate::app::keymap::normal_binds(app) {
        if binding.help {
            bind(lines, binding.label, binding.desc);
        }
    }
    if app.active_view == crate::app::View::Esvs {
        group(lines, "ESV Views");
        bind(lines, "Variables", "ESV variable values and descriptions");
        bind(
            lines,
            "Secrets",
            "ESV secret values, versions, and descriptions",
        );
        if app
            .active_tenant()
            .is_some_and(|tenant| tenant.allows_secret_mappings())
        {
            bind(
                lines,
                "Mappings",
                "AM secret-label aliases; sandbox/development tenants only",
            );
        } else {
            bind(
                lines,
                "Mappings",
                "hidden on staging/production; promoted from lower environments",
            );
        }
    }
    bind(lines, "q / ^C", "quit");
    bind(lines, "F1/?", "show keybinds");
}

fn selector_lines(lines: &mut Vec<Line<'static>>) {
    group(lines, "Function selector");
    bind(lines, "Type", "filter functions");
    bind(lines, "Backspace", "delete character");
    bind(lines, "↑/↓", "move selection");
    bind(lines, "Enter", "open selected function");
    bind(lines, "Esc", "cancel");
}

fn esv_lines(app: &App, mode: crate::esv::screen::Mode, lines: &mut Vec<Line<'static>>) {
    if let Some(entries) = crate::esv::screen::help_lines(mode, app) {
        feature_lines(Some(entries), lines);
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
    bind(lines, "↑/↓", "move selection");
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

fn onboard_lines(app: &App, mode: OnboardMode, lines: &mut Vec<Line<'static>>) {
    if let Some(entries) = crate::onboard::screen::help_lines(mode, app.has_env_creds) {
        feature_lines(Some(entries), lines);
    }
}

fn env_picker_lines(app: &App, lines: &mut Vec<Line<'static>>) {
    group(lines, "Switch Tenant");
    bind(lines, "Enter", "switch to selected tenant");
    bind(lines, "Esc", "cancel");
    group(lines, "Movement");
    bind(lines, "↑/↓", "move selection");
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
    bind(lines, "↑/↓", "move selection");
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

fn feature_lines(
    entries: Option<Vec<(&'static str, &'static str)>>,
    lines: &mut Vec<Line<'static>>,
) {
    if let Some(entries) = entries {
        for (key, desc) in entries.iter() {
            bind(lines, *key, *desc);
        }
    }
}

fn number_range(count: usize) -> Option<String> {
    let max = count.min(9);
    match max {
        0 => None,
        1 => Some("1".to_string()),
        _ => Some(format!("1-{max}")),
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
