//! Global draw root: dispatches on `InputMode` — full-screen feature modals
//! first, then the dashboard (header + active view body + footer hints).
//! One arm per feature; feature rendering lives in each vertical's `view`.

use crate::app::{env_picker, selector};
use crate::tui::{header, keybind_help, popup_confirm, toast};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::app::{App, InputMode};
use crate::esv::screen::Mode as EsvMode;
use crate::managed::screen::Mode as ManagedMode;
use crate::secretmap::screen::Mode as SecretmapMode;
use crate::secrets::{screen::Mode as SecretsMode, view as secret};

pub fn draw(f: &mut Frame, app: &App) {
    // Every modal owns the whole screen. The dashboard (Normal + ESV search)
    // is the only thing that gets the header / body / global-hints layout.
    match app.input_mode {
        InputMode::Vault(crate::vault::screen::Mode::Relock) => {}
        InputMode::Vault(mode) => {
            crate::vault::draw(f, app, mode);
            return;
        }
        InputMode::Onboard(mode) => {
            crate::onboard::view::draw(f, app, mode);
            draw_keybind_help(f, app);
            toast::draw(f, app);
            return;
        }
        InputMode::EnvPicker => {
            env_picker::draw(f, app);
            draw_keybind_help(f, app);
            toast::draw(f, app);
            return;
        }
        InputMode::Selector => {
            selector::draw(f, app);
            draw_keybind_help(f, app);
            toast::draw(f, app);
            return;
        }
        InputMode::ProdConfirm => {
            crate::app::prod_confirm::draw(f, app);
            draw_keybind_help(f, app);
            toast::draw(f, app);
            return;
        }
        InputMode::UndoHistory => {
            crate::undo::view::draw(f, app);
            draw_keybind_help(f, app);
            toast::draw(f, app);
            return;
        }
        InputMode::Normal
        | InputMode::Esv(_)
        | InputMode::Secrets(_)
        | InputMode::Scripts(_)
        | InputMode::Managed(_)
        | InputMode::Mappings(_)
        | InputMode::Access(_)
        | InputMode::IdmStore(_)
        | InputMode::Oauth(_)
        | InputMode::Secretmap(_) => {}
    }

    let area = f.area();
    let show_hints = !app.keybind_help_open
        && !matches!(
            app.input_mode,
            InputMode::Esv(EsvMode::RestartConfirm | EsvMode::DeleteConfirm)
                | InputMode::Managed(ManagedMode::DeleteFieldConfirm)
                | InputMode::Secretmap(SecretmapMode::DeleteConfirm)
                | InputMode::Secrets(
                    SecretsMode::AddVersion
                        | SecretsMode::DeleteConfirm
                        | SecretsMode::VersionDestroyConfirm
                )
        );
    let chunks = Layout::vertical([
        Constraint::Length(1),                              // top: view + chips
        Constraint::Length(1),                              // breathing room under the header
        Constraint::Min(0),                                 // body
        Constraint::Length(if show_hints { 1 } else { 0 }), // bottom: keybind hints
    ])
    .split(area);

    header::draw(f, app, chunks[0]);
    draw_body(f, app, chunks[2]);
    if show_hints {
        header::draw_hints(f, app, chunks[3]);
    }

    // Overlay popup confirm for restart, drawn on top of the dashboard
    // (not full-screen — short y/n questions get the small popup style).
    if app.input_mode == InputMode::Esv(EsvMode::RestartConfirm) {
        let n = app
            .active_tenant()
            .map(|t| crate::esv::state::pending_count(app, &t.name))
            .unwrap_or(0);
        let noun = if n == 1 { "change" } else { "changes" };
        let message = format!(
            "{n} {noun} pending.\n\nApply by restarting the tenant runtime?\nTakes a few minutes; users already signed in stay signed in."
        );
        popup_confirm::draw(f, "Apply pending changes?", &message);
    }
    if app.input_mode == InputMode::Esv(EsvMode::DeleteConfirm) {
        let id = app
            .esv_matches()
            .get(app.esv.list.selected)
            .map(|m| m.id.clone())
            .unwrap_or_else(|| "selected variable".to_string());
        let message = format!("Delete {id}?\n\nThis can be undone from the undo log.");
        popup_confirm::draw(f, "Delete ESV variable?", &message);
    }

    // Secret overlays. The version panel now lives in the detail pane (drawn
    // by `secret::draw_body`), so these are just the create / add-version
    // forms and the two y/n confirmations — each drawn over that panel.
    match app.input_mode {
        InputMode::Vault(crate::vault::screen::Mode::Relock) => {
            crate::vault::draw(f, app, crate::vault::screen::Mode::Relock)
        }
        InputMode::Managed(ManagedMode::DeleteFieldConfirm) => {
            crate::managed::view::draw_delete_field_confirm(f, app);
        }
        InputMode::Secrets(SecretsMode::AddVersion) => secret::draw_add_version(f, app),
        InputMode::Secrets(SecretsMode::VersionDestroyConfirm) => {
            let version = app
                .secret
                .pending_version_destroy
                .as_ref()
                .map(|(_, _, v)| v.clone())
                .unwrap_or_default();
            let message = format!(
                "Destroy version {version}?\n\nThis is irreversible — the version's value is gone for good."
            );
            popup_confirm::draw(f, "Destroy secret version?", &message);
        }
        InputMode::Secrets(SecretsMode::DeleteConfirm) => {
            let id = app
                .secret
                .pending_delete
                .as_ref()
                .map(|p| p.id.clone())
                .unwrap_or_else(|| "selected secret".to_string());
            let message = format!(
                "Delete secret {id} and ALL its versions?\n\nThis is irreversible — secret values cannot be recovered."
            );
            popup_confirm::draw(f, "Delete secret?", &message);
        }
        InputMode::Secretmap(SecretmapMode::DeleteConfirm) => {
            let (id, alias) = app
                .secretmap
                .pending_delete
                .as_ref()
                .map(|pending| (pending.secret_id.clone(), pending.prior_alias.clone()))
                .unwrap_or_else(|| ("selected mapping".to_string(), "unknown".to_string()));
            let message = format!(
                "Remove mapping {id}?\n\nCurrent alias: {alias}\nThis can be undone from the undo log."
            );
            popup_confirm::draw(f, "Remove secret mapping?", &message);
        }
        _ => {}
    }

    draw_keybind_help(f, app);
    toast::draw(f, app);
}

fn draw_keybind_help(f: &mut Frame, app: &App) {
    if app.keybind_help_open {
        keybind_help::draw(f, app);
    }
}

fn draw_body(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    if app.tenants.is_empty() {
        let chunks = Layout::vertical([
            Constraint::Percentage(40),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .split(area);

        let mut lines = vec![
            Line::from(Span::styled(
                "Welcome to pingone-aic-manager",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "No tenants configured. Press Ctrl-T to add your first tenant.",
                Style::default().fg(Color::Gray),
            )),
        ];
        lines.push(Line::from(Span::styled(
            "Press Ctrl-A to manage authentication factors.",
            Style::default().fg(Color::Gray),
        )));
        f.render_widget(
            Paragraph::new(lines).alignment(Alignment::Center),
            chunks[1],
        );
    } else if app.active_view == crate::app::View::Scripts {
        crate::scripts::view::draw_body(f, app, area);
    } else if app.active_view == crate::app::View::Managed {
        crate::managed::view::draw_body(f, app, area);
    } else if app.active_view == crate::app::View::Mappings {
        crate::mappings::view::draw_body(f, app, area);
    } else if app.active_view == crate::app::View::Access {
        crate::access::view::draw_body(f, app, area);
    } else if app.active_view == crate::app::View::IdmStore {
        crate::idmstore::view::draw_body(f, app, area);
    } else if app.active_view == crate::app::View::Oauth {
        crate::oauth::view::draw_body(f, app, area);
    } else {
        crate::esv::view::draw(f, app, area);
    }
}
