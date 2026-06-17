use ratatui::{
    Frame,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, ListState, Paragraph},
};

use crate::app::App;
use crate::tui::modal_chrome::Modal;
use crate::undo::{Capability, EntryStatus, Sensitivity};

pub fn draw(f: &mut Frame, app: &App) {
    let summaries = crate::undo::screen::summaries(app);
    let body = Modal {
        title: "Undo History",
        status: app.active_tenant().map(|tenant| tenant.name.as_str()),
        hints: &[("↑/↓", "navigate"), ("Enter", "undo"), ("Esc", "close")],
        body_height: summaries.len().max(1) as u16,
    }
    .draw(f, f.area());

    if summaries.is_empty() {
        f.render_widget(
            Paragraph::new(Span::styled(
                "No undo entries for this tenant.",
                Style::default().fg(Color::DarkGray),
            )),
            body,
        );
        return;
    }

    let items: Vec<ListItem> = summaries
        .iter()
        .map(|summary| {
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{}  ", summary.created_at.format("%H:%M:%S")),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("{:<7} ", status_label(summary.status)),
                    status_style(summary.status),
                ),
                Span::styled(
                    format!("{:<10} ", capability_label(summary.capability)),
                    Style::default().fg(Color::Blue),
                ),
                Span::styled(
                    format!("{:<12} ", sensitivity_label(summary.sensitivity)),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::raw(summary.description.clone()),
            ]))
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(
        app.undo_history_idx.min(summaries.len().saturating_sub(1)),
    ));
    let list = List::new(items)
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");
    f.render_stateful_widget(list, body, &mut state);
}

fn status_label(status: EntryStatus) -> &'static str {
    match status {
        EntryStatus::Pending => "pending",
        EntryStatus::AppliedSuccess => "done",
        EntryStatus::AppliedConflict => "conflict",
        EntryStatus::AppliedFailure => "failed",
        EntryStatus::Expired => "expired",
    }
}

fn status_style(status: EntryStatus) -> Style {
    match status {
        EntryStatus::Pending => Style::default().fg(Color::Green),
        EntryStatus::AppliedSuccess => Style::default().fg(Color::DarkGray),
        EntryStatus::AppliedConflict | EntryStatus::AppliedFailure => {
            Style::default().fg(Color::Red)
        }
        EntryStatus::Expired => Style::default().fg(Color::DarkGray),
    }
}

fn capability_label(capability: Capability) -> &'static str {
    match capability {
        Capability::Undoable => "undoable",
        Capability::BestEffort => "best",
        Capability::Irreversible => "no-undo",
    }
}

fn sensitivity_label(sensitivity: Sensitivity) -> &'static str {
    match sensitivity {
        Sensitivity::PublicMetadata => "public",
        Sensitivity::TenantConfig => "tenant",
        Sensitivity::SensitiveValue => "sensitive",
        Sensitivity::SecretValue => "secret",
    }
}
