//! Read-only managed-object browser: searchable object list and schema detail.

use std::collections::HashSet;

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use serde_json::Value;

use crate::app::{App, InputMode};
use crate::managed::api::ObjectSummary;
use crate::managed::screen::Mode;
use crate::managed::state::{
    AddFieldFocus, AddKind, DeleteObjectState, EditFieldFocus, FieldAttr, LoadState, ManagedMatch,
    NewObjectFocus, RefPropFocus, RelationshipFocus,
};
use crate::tui::widgets::draw_bool_row;

pub fn draw_body(f: &mut Frame, app: &App, area: Rect) {
    let Some(tenant) = app.active_tenant().map(|tenant| tenant.name.clone()) else {
        return;
    };

    let doc = match app.managed.data.get(&tenant) {
        None | Some(LoadState::Loading) => {
            crate::tui::list_chrome::draw_status_line(
                f,
                area,
                "  Loading managed objects…",
                Color::DarkGray,
            );
            return;
        }
        Some(LoadState::Failed(error)) => {
            crate::tui::list_chrome::draw_status_line(
                f,
                area,
                &format!("  Managed objects failed: {error}"),
                Color::Red,
            );
            return;
        }
        Some(LoadState::Loaded(doc)) => doc,
    };
    let summaries = match crate::managed::api::summarize(doc) {
        Ok(summaries) if summaries.is_empty() => {
            if app.input_mode == InputMode::Managed(Mode::NewObject)
                && app.managed.new_object.is_some()
            {
                draw_new_object_form(f, app, area);
                return;
            }
            crate::tui::list_chrome::draw_status_line(
                f,
                area,
                "  No managed objects found.",
                Color::DarkGray,
            );
            return;
        }
        Ok(summaries) => summaries,
        Err(error) => {
            crate::tui::list_chrome::draw_status_line(
                f,
                area,
                &format!("  Managed schema failed: {error}"),
                Color::Red,
            );
            return;
        }
    };

    let matches = app.managed.matches(Some(&tenant));
    let columns =
        Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)]).split(area);
    draw_list(f, app, summaries.len(), &matches, columns[0]);
    draw_detail(f, app, doc, &summaries, &matches, columns[1]);
}

fn draw_list(f: &mut Frame, app: &App, total: usize, matches: &[ManagedMatch], area: Rect) {
    let searching = app.input_mode == InputMode::Managed(Mode::Search);
    let count_text = if app.managed.query.is_empty() {
        format!("{total} objects ")
    } else {
        format!("{}/{} objects ", matches.len(), total)
    };
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);
    crate::tui::list_chrome::draw_search_row(
        f,
        rows[0],
        &app.managed.query,
        searching,
        &count_text,
    );

    let height = rows[1].height as usize;
    let selected = app.managed.selected.min(matches.len().saturating_sub(1));
    let scroll =
        crate::tui::list_chrome::clamp_scroll(app.managed.scroll, selected, height, matches.len());
    let lines: Vec<Line> = matches
        .iter()
        .enumerate()
        .skip(scroll)
        .take(height)
        .map(|(idx, item)| {
            let failed = app
                .active_tenant()
                .map(|tenant| {
                    app.managed
                        .failed_writes
                        .contains(&(tenant.name.clone(), item.name.clone()))
                })
                .unwrap_or(false);
            let saving = app
                .active_tenant()
                .map(|tenant| {
                    app.managed
                        .in_flight_writes
                        .contains(&(tenant.name.clone(), item.name.clone()))
                })
                .unwrap_or(false);
            render_row(item, idx == selected, failed, saving)
        })
        .collect();
    f.render_widget(Paragraph::new(lines), rows[1]);
}

fn render_row(item: &ManagedMatch, selected: bool, failed: bool, saving: bool) -> Line<'static> {
    let row_style = if selected {
        Style::default()
            .fg(if failed { Color::Red } else { Color::Black })
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else if failed {
        Style::default().fg(Color::Red)
    } else {
        Style::default().fg(Color::Gray)
    };
    let match_style = if selected {
        row_style.add_modifier(Modifier::UNDERLINED)
    } else {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    };
    let suffix_style = if selected {
        row_style
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let mut spans = vec![Span::styled(if selected { "▶ " } else { "  " }, row_style)];

    if item.positions.is_empty() {
        spans.push(Span::styled(item.name.clone(), row_style));
    } else {
        let mut positions = item.positions.iter().copied().peekable();
        for (idx, ch) in item.name.chars().enumerate() {
            if positions
                .peek()
                .copied()
                .is_some_and(|position| position as usize == idx)
            {
                positions.next();
                spans.push(Span::styled(ch.to_string(), match_style));
            } else {
                spans.push(Span::styled(ch.to_string(), row_style));
            }
        }
    }

    spans.push(Span::styled(
        format!("  {} props", item.properties),
        suffix_style,
    ));
    if item.hooks_inline > 0 {
        spans.push(Span::styled(
            format!(" · {} hooks", item.hooks_inline),
            suffix_style,
        ));
    }
    if saving {
        spans.push(Span::styled(
            " · saving",
            Style::default().fg(Color::Yellow),
        ));
    } else if failed {
        spans.push(Span::styled(" · failed", Style::default().fg(Color::Red)));
    }
    Line::from(spans)
}

fn draw_detail(
    f: &mut Frame,
    app: &App,
    doc: &Value,
    summaries: &[ObjectSummary],
    matches: &[ManagedMatch],
    area: Rect,
) {
    let block = Block::default()
        .borders(Borders::LEFT)
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let inner = Rect {
        x: inner.x + 2,
        y: inner.y,
        width: inner.width.saturating_sub(2),
        height: inner.height,
    };

    if app.input_mode == InputMode::Managed(Mode::NewObject) && app.managed.new_object.is_some() {
        draw_new_object_form(f, app, inner);
        return;
    }

    let selected = app.managed.selected.min(matches.len().saturating_sub(1));
    let Some(item) = matches.get(selected) else {
        crate::tui::list_chrome::draw_status_line(f, inner, "no match", Color::DarkGray);
        return;
    };
    let Some(summary) = summaries.get(item.idx) else {
        return;
    };
    let Ok(object) = crate::managed::api::object_named(doc, &item.name) else {
        return;
    };

    match app.input_mode {
        InputMode::Managed(Mode::AddChooseKind) if app.managed.add_choose.is_some() => {
            draw_add_kind_chooser(f, app, inner);
            return;
        }
        InputMode::Managed(Mode::EditField | Mode::EnumNarrowConfirm)
            if app.managed.editing.is_some() =>
        {
            // The warning floats over the form it is about, so the user can see
            // the values they are dropping while deciding.
            draw_edit_field_form(f, app, inner);
            if app.input_mode == InputMode::Managed(Mode::EnumNarrowConfirm) {
                draw_enum_narrow_confirm(f, app);
            }
            return;
        }
        InputMode::Managed(Mode::AddField) if app.managed.add_field.is_some() => {
            draw_add_field_form(f, app, inner);
            return;
        }
        InputMode::Managed(Mode::Relationship | Mode::RelationshipTarget | Mode::RefProp)
            if app.managed.relationship_form.is_some() =>
        {
            draw_relationship_form(f, app, inner);
            if app.input_mode == InputMode::Managed(Mode::RelationshipTarget) {
                draw_relationship_target_picker(f, app, inner);
            }
            if app.input_mode == InputMode::Managed(Mode::RefProp)
                && app.managed.ref_prop_draft.is_some()
            {
                draw_ref_prop_form(f, app, inner);
            }
            return;
        }
        InputMode::Managed(Mode::AddHook) if app.managed.add_hook.is_some() => {
            draw_add_hook_picker(f, app, inner);
            return;
        }
        InputMode::Managed(Mode::RenameField) if app.managed.renaming.is_some() => {
            draw_rename_field_form(f, app, inner);
            return;
        }
        InputMode::Managed(Mode::RenameObject) if app.managed.renaming_object.is_some() => {
            draw_rename_object_form(f, app, inner);
            return;
        }
        InputMode::Managed(Mode::RenameObjectConfirm)
            if app.managed.rename_object_confirm.is_some() =>
        {
            draw_rename_object_confirm(f, app, inner);
            return;
        }
        InputMode::Managed(Mode::DeleteObjectConfirm)
            if app.managed.pending_object_delete.is_some() =>
        {
            draw_delete_object_confirm(f, app, inner);
            return;
        }
        _ => {}
    }

    let mut lines = vec![
        Line::from(Span::styled(
            item.name.clone(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            format!(
                "{} properties · {} inline hooks · {} file hooks",
                item.properties, item.hooks_inline, item.hooks_file
            ),
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(Span::styled(
            "Enter edit selected field · [ ] change field",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(""),
    ];

    let properties = object
        .pointer("/schema/properties")
        .and_then(Value::as_object);
    let required: HashSet<String> = crate::managed::state::required_fields(object);
    let property_names = crate::managed::state::property_names(object);
    let selected_property = app
        .managed
        .property_selected
        .min(property_names.len().saturating_sub(1));

    let hook_lines = hook_lines(summary);
    let remaining = (inner.height as usize).saturating_sub(lines.len());
    let property_slots = remaining.saturating_sub(hook_lines.len().saturating_add(1));
    let shown_properties = if property_names.len() > property_slots {
        property_slots.saturating_sub(1)
    } else {
        property_names.len()
    };
    let property_scroll = crate::tui::list_chrome::clamp_scroll(
        0,
        selected_property,
        shown_properties,
        property_names.len(),
    );
    if let Some(properties) = properties {
        for (idx, name) in property_names
            .iter()
            .enumerate()
            .skip(property_scroll)
            .take(shown_properties)
        {
            lines.push(property_line(
                name,
                &properties[name],
                required.contains(name),
                idx == selected_property,
                inner.width as usize,
            ));
        }
    }
    if property_names.len() > shown_properties && property_slots > 0 {
        lines.push(Line::from(Span::styled(
            format!("… (+{} more)", property_names.len() - shown_properties),
            Style::default().fg(Color::DarkGray),
        )));
    }

    if lines.len() < inner.height as usize {
        lines.push(Line::from(""));
    }
    lines.extend(hook_lines);
    lines.truncate(inner.height as usize);
    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_add_kind_chooser(f: &mut Frame, app: &App, area: Rect) {
    let Some(draft) = app.managed.add_choose.as_ref() else {
        return;
    };
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(area);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "Add managed property",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ))),
        rows[0],
    );
    draw_selector_row(
        f,
        rows[1],
        "Kind",
        match draft.kind {
            AddKind::Field => "Field",
            AddKind::Relationship => "Relationship",
        },
        true,
    );
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "Enter continues to the selected form",
            Style::default().fg(Color::DarkGray),
        ))),
        rows[2],
    );
}

fn draw_rename_field_form(f: &mut Frame, app: &App, area: Rect) {
    let Some(rename) = app.managed.renaming.as_ref() else {
        return;
    };
    let error_h = if rename.error.is_some() { 2 } else { 0 };
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Length(error_h),
    ])
    .split(area);
    form_title(f, rows[0], &rename.object_name, "rename field");
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!("Current key  {}", rename.old_key),
            Style::default().fg(Color::DarkGray),
        ))),
        rows[1],
    );
    rename.key.draw(
        f,
        rows[2],
        rename.focused == crate::managed::state::RenameFieldFocus::Key,
    );
    draw_form_error(f, rows[3], rename.error.as_deref());
}

fn draw_rename_object_form(f: &mut Frame, app: &App, area: Rect) {
    let Some(rename) = app.managed.renaming_object.as_ref() else {
        return;
    };
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Length(2),
    ])
    .split(area);
    form_title(f, rows[0], &rename.old_name, "rename object");
    f.render_widget(
        Paragraph::new("Renaming does not migrate records."),
        rows[1],
    );
    rename.key.draw(f, rows[2], true);
    draw_form_error(f, rows[3], rename.error.as_deref());
}

fn draw_new_object_form(f: &mut Frame, app: &App, area: Rect) {
    let Some(draft) = app.managed.new_object.as_ref() else {
        return;
    };
    let error_h = if draft.error.is_some() { 2 } else { 0 };
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Length(2),
        Constraint::Min(4),
        Constraint::Length(error_h),
        Constraint::Length(2),
    ])
    .split(area);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "new object",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ))),
        rows[0],
    );
    draft
        .name
        .draw(f, rows[1], draft.focused == NewObjectFocus::Name);
    draft
        .title
        .draw(f, rows[2], draft.focused == NewObjectFocus::Title);
    draft
        .description
        .draw(f, rows[3], draft.focused == NewObjectFocus::Description);
    draw_form_error(f, rows[4], draft.error.as_deref());
    draw_save_button(f, rows[5], draft.focused == NewObjectFocus::Save);
}

fn draw_rename_object_confirm(f: &mut Frame, app: &App, area: Rect) {
    let Some(confirm) = app.managed.rename_object_confirm.as_ref() else {
        return;
    };
    let records = confirm.record_count.map_or_else(|| "Record count unknown".to_string(), |count| format!("{count} record(s) will be ORPHANED in the old backend (recoverable by renaming back)"));
    let lines = vec![
        Line::from(format!(
            "Rename {} → {}",
            confirm.draft.old_name, confirm.draft.key.value
        )),
        Line::from(format!(
            "{} relationship reference(s) will be repointed",
            confirm.repoints
        )),
        Line::from(records),
        Line::from("config/sync mappings are not rewritten"),
        Line::from("Press y to proceed, n or Esc to cancel"),
    ];
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), area);
}

fn draw_delete_object_confirm(f: &mut Frame, app: &App, area: Rect) {
    let Some(state) = app.managed.pending_object_delete.as_ref() else {
        return;
    };
    let lines: Vec<Line> = delete_object_warning(state)
        .into_iter()
        .map(Line::from)
        .collect();
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), area);
}

/// Confirm-modal copy. The record-fate wording rests on a live probe
/// (2026-07-31, `docs/api/10-managed-objects.md`): removing an object from
/// `config/managed` detaches its records from the API without destroying them,
/// and restoring the config brings every record back under its original `_id`.
/// That is what makes `^Z` a genuine recovery rather than a partial one.
fn delete_object_warning(state: &DeleteObjectState) -> Vec<String> {
    let records = match &state.record_count {
        None => "Record count: counting…".to_string(),
        Some(Ok(count)) => format!(
            "{count} record(s) detach from the API but are not destroyed — undo restores them"
        ),
        Some(Err(error)) => format!("Record count unavailable: {error}"),
    };
    let mut lines = vec![
        format!("Delete managed object {}", state.object_name),
        records,
    ];
    if state.inbound.is_empty() {
        lines.push("No inbound relationships will be removed".to_string());
    } else {
        lines.push("Inbound relationships to remove:".to_string());
        lines.extend(
            state
                .inbound
                .iter()
                .map(|(object, property)| format!("{object}.{property}")),
        );
    }
    lines.push("This can be undone from the undo log.".to_string());
    lines.push("Press y to proceed, n or Esc to cancel".to_string());
    lines
}

fn property_line(
    name: &str,
    property: &Value,
    required: bool,
    selected: bool,
    max_width: usize,
) -> Line<'static> {
    let base_style = if selected {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    let mut spans = vec![Span::styled(if selected { "▶ " } else { "  " }, base_style)];
    spans.push(Span::styled(name.to_string(), base_style));
    if required {
        spans.push(Span::styled(
            "*",
            if selected {
                base_style
            } else {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            },
        ));
    }
    spans.push(Span::styled(
        ": ",
        if selected {
            base_style
        } else {
            Style::default().fg(Color::DarkGray)
        },
    ));
    let property_type = crate::managed::state::property_type(property);
    spans.push(Span::styled(
        property_type.clone(),
        if selected {
            base_style
        } else {
            Style::default().fg(Color::White)
        },
    ));
    if let Some(constraint) = crate::managed::ops::property_enum(property) {
        let values = constraint
            .values
            .iter()
            .map(|value| value.value.as_str())
            .collect::<Vec<_>>()
            .join("|");
        let prefix_width =
            name.chars().count() + property_type.chars().count() + 4 + usize::from(required);
        let available = max_width.saturating_sub(prefix_width + 3);
        let metadata = truncate_metadata(&values, available);
        spans.push(Span::styled(
            format!(" ({metadata})"),
            if selected {
                base_style
            } else {
                Style::default().fg(Color::DarkGray)
            },
        ));
    }
    Line::from(spans)
}

fn truncate_metadata(value: &str, max_width: usize) -> String {
    if value.chars().count() <= max_width {
        return value.to_string();
    }
    if max_width <= 1 {
        return "…".to_string();
    }
    format!("{}…", value.chars().take(max_width - 1).collect::<String>())
}

fn hook_lines(summary: &ObjectSummary) -> Vec<Line<'static>> {
    if summary.hooks_inline.is_empty() && summary.hooks_file.is_empty() {
        return vec![Line::from(Span::styled(
            "(no inline hooks)",
            Style::default().fg(Color::DarkGray),
        ))];
    }

    let mut lines = Vec::new();
    for name in &summary.hooks_inline {
        lines.push(Line::from(vec![
            Span::styled("hook  ", Style::default().fg(Color::DarkGray)),
            Span::styled(name.clone(), Style::default().fg(Color::Green)),
            Span::styled(
                format!("  (sync: aic script pull managed/{}.{name})", summary.name),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }
    for name in &summary.hooks_file {
        lines.push(Line::from(vec![
            Span::styled("hook  ", Style::default().fg(Color::DarkGray)),
            Span::styled(name.clone(), Style::default().fg(Color::DarkGray)),
            Span::styled("  (file, read-only)", Style::default().fg(Color::DarkGray)),
        ]));
    }
    lines
}

fn draw_edit_field_form(f: &mut Frame, app: &App, area: Rect) {
    let Some(edit) = app.managed.editing.as_ref() else {
        return;
    };

    let error_h = if edit.default_value.error().is_some() || edit.error.is_some() {
        2
    } else {
        0
    };
    let enum_eligible = crate::managed::ops::enum_constraint_eligible(&edit.original_property);
    let rows = Layout::vertical([
        Constraint::Length(1), // field id
        Constraint::Length(1), // type/capability
        Constraint::Length(1), // gap
        Constraint::Length(2), // key
        Constraint::Length(1),
        Constraint::Length(2), // title
        Constraint::Length(1),
        Constraint::Min(4), // description
        Constraint::Length(1),
        Constraint::Length(if enum_eligible { 2 } else { 0 }),
        Constraint::Length(if enum_eligible { 1 } else { 0 }),
        Constraint::Length(2), // default
        Constraint::Length(1),
        Constraint::Length(1), // required
        Constraint::Length(1), // searchable
        Constraint::Length(1), // viewable
        Constraint::Length(1), // userEditable
        Constraint::Length(error_h),
        Constraint::Length(2), // save
    ])
    .split(area);

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!("{}.{}", edit.object_name, edit.field_key),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  edit attributes", Style::default().fg(Color::DarkGray)),
        ])),
        rows[0],
    );
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Type  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                edit.property_type.clone(),
                Style::default().fg(Color::White),
            ),
            Span::styled("  ·  ", Style::default().fg(Color::DarkGray)),
            Span::styled(caps_label(edit.caps), Style::default().fg(Color::DarkGray)),
        ])),
        rows[1],
    );

    edit.key
        .draw(f, rows[3], edit.focused == EditFieldFocus::Key);
    if !edit.caps.rename_key {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "Key rename unavailable for this field",
                Style::default().fg(Color::DarkGray),
            ))),
            rows[4],
        );
    }
    edit.title
        .draw(f, rows[5], edit.focused == EditFieldFocus::Title);
    edit.description
        .draw(f, rows[7], edit.focused == EditFieldFocus::Description);

    draw_bool_row(
        f,
        rows[13],
        "Required",
        Some(edit.required),
        edit.focused == EditFieldFocus::Required,
        edit.caps.can_edit_attr(FieldAttr::Required),
    );
    draw_bool_row(
        f,
        rows[14],
        "Searchable",
        Some(edit.searchable),
        edit.focused == EditFieldFocus::Searchable,
        edit.caps.can_edit_attr(FieldAttr::Searchable),
    );
    draw_bool_row(
        f,
        rows[15],
        "Viewable",
        Some(edit.viewable),
        edit.focused == EditFieldFocus::Viewable,
        edit.caps.can_edit_attr(FieldAttr::Viewable),
    );
    draw_bool_row(
        f,
        rows[16],
        "User editable",
        Some(edit.user_editable),
        edit.focused == EditFieldFocus::UserEditable,
        edit.caps.can_edit_attr(FieldAttr::UserEditable),
    );

    if let Some(error) = edit.default_value.error().or(edit.error.as_deref()) {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                error.to_string(),
                Style::default().fg(Color::Yellow),
            )))
            .wrap(Wrap { trim: false }),
            rows[17],
        );
    }
    if enum_eligible {
        edit.enum_values
            .draw(f, rows[9], edit.focused == EditFieldFocus::Enum);
    }
    edit.default_value
        .draw(f, rows[11], edit.focused == EditFieldFocus::Default);
    draw_save_button(f, rows[18], edit.focused == EditFieldFocus::Save);
}

fn draw_add_field_form(f: &mut Frame, app: &App, area: Rect) {
    let Some(draft) = app.managed.add_field.as_ref() else {
        return;
    };
    let error_h = if draft.default_value.error().is_some() || draft.error.is_some() {
        2
    } else {
        0
    };
    let enum_eligible = draft.enum_eligible();
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Min(4),
        Constraint::Length(1),
        Constraint::Length(1), // type
        Constraint::Length(if enum_eligible { 2 } else { 0 }),
        Constraint::Length(if enum_eligible { 1 } else { 0 }),
        Constraint::Length(2), // default
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(error_h),
        Constraint::Length(2),
    ])
    .split(area);

    form_title(f, rows[0], &draft.object_name, "add scalar field");
    draft
        .key
        .draw(f, rows[2], draft.focused == AddFieldFocus::Key);
    draft
        .title
        .draw(f, rows[4], draft.focused == AddFieldFocus::Title);
    draft
        .description
        .draw(f, rows[6], draft.focused == AddFieldFocus::Description);
    draw_selector_row(
        f,
        rows[8],
        "Type",
        draft.field_type().label(),
        draft.focused == AddFieldFocus::Type,
    );
    if enum_eligible {
        draft
            .enum_values
            .draw(f, rows[9], draft.focused == AddFieldFocus::Enum);
    }
    draft
        .default_value
        .draw(f, rows[11], draft.focused == AddFieldFocus::Default);
    draw_bool_row(
        f,
        rows[13],
        "Searchable",
        Some(draft.searchable),
        draft.focused == AddFieldFocus::Searchable,
        true,
    );
    draw_bool_row(
        f,
        rows[14],
        "Viewable",
        Some(draft.viewable),
        draft.focused == AddFieldFocus::Viewable,
        true,
    );
    draw_bool_row(
        f,
        rows[15],
        "User editable",
        Some(draft.user_editable),
        draft.focused == AddFieldFocus::UserEditable,
        true,
    );
    draw_bool_row(
        f,
        rows[16],
        "Required",
        Some(draft.required),
        draft.focused == AddFieldFocus::Required,
        true,
    );
    draw_form_error(
        f,
        rows[17],
        draft.default_value.error().or(draft.error.as_deref()),
    );
    draw_save_button(f, rows[18], draft.focused == AddFieldFocus::Save);
}

fn draw_relationship_form(f: &mut Frame, app: &App, area: Rect) {
    let Some(draft) = app.managed.relationship_form.as_ref() else {
        return;
    };
    let error_h = if draft.error.is_some() { 2 } else { 0 };
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        // A single-line TextField needs two rows (label + value); one row renders
        // the label only, leaving no visible input. Collapses to 0 when there is
        // no reverse relationship to name.
        Constraint::Length(
            if draft.reverse != crate::managed::state::ReverseCardinality::None {
                2
            } else {
                0
            },
        ),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Length(error_h),
        Constraint::Length(2),
    ])
    .split(area);

    form_title(
        f,
        rows[0],
        &draft.source_object,
        if draft.previous.is_some() {
            "edit relationship"
        } else {
            "add relationship"
        },
    );
    draft
        .key
        .draw(f, rows[2], draft.focused == RelationshipFocus::Key);
    draft
        .title
        .draw(f, rows[4], draft.focused == RelationshipFocus::Title);
    draft
        .description
        .draw(f, rows[6], draft.focused == RelationshipFocus::Description);
    draw_target_row(
        f,
        rows[8],
        draft.target_name.as_deref().unwrap_or("(choose target)"),
        draft.focused == RelationshipFocus::Target,
    );
    draw_selector_row(
        f,
        rows[9],
        "Forward",
        draft.forward.label(),
        draft.focused == RelationshipFocus::Forward,
    );
    draw_selector_row(
        f,
        rows[10],
        "Reverse",
        draft.reverse.label(),
        draft.focused == RelationshipFocus::Reverse,
    );
    if draft.reverse != crate::managed::state::ReverseCardinality::None {
        draft
            .reverse_key
            .draw(f, rows[11], draft.focused == RelationshipFocus::ReverseKey);
    }
    draw_bool_row(
        f,
        rows[12],
        "Searchable",
        Some(draft.searchable),
        draft.focused == RelationshipFocus::Searchable,
        true,
    );
    draw_bool_row(
        f,
        rows[13],
        "Viewable",
        Some(draft.viewable),
        draft.focused == RelationshipFocus::Viewable,
        true,
    );
    draw_bool_row(
        f,
        rows[14],
        "User editable",
        Some(draft.user_editable),
        draft.focused == RelationshipFocus::UserEditable,
        true,
    );
    draw_bool_row(
        f,
        rows[15],
        "Required",
        Some(draft.required),
        draft.focused == RelationshipFocus::Required,
        true,
    );
    draw_bool_row(
        f,
        rows[16],
        "Validate",
        Some(draft.validate),
        draft.focused == RelationshipFocus::Validate,
        true,
    );
    f.render_widget(
        Paragraph::new("Custom _refProperties").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        rows[17],
    );
    let selected = draft
        .ref_selected
        .min(draft.ref_properties.len().saturating_sub(1));
    let lines = if draft.ref_properties.is_empty() {
        vec![Line::from(Span::styled(
            "  (none)",
            Style::default().fg(Color::DarkGray),
        ))]
    } else {
        draft
            .ref_properties
            .iter()
            .enumerate()
            .map(|(index, property)| {
                simple_pick_line(
                    &format!(
                        "{}: {}  {}",
                        property.name,
                        property.kind.label(),
                        property.label
                    ),
                    draft.focused == RelationshipFocus::RefProperties && index == selected,
                )
            })
            .collect()
    };
    f.render_widget(Paragraph::new(lines), rows[18]);
    f.render_widget(
        Paragraph::new("Ctrl-A add · Enter edit · d delete")
            .style(Style::default().fg(Color::DarkGray)),
        rows[19],
    );
    draw_form_error(f, rows[20], draft.error.as_deref());
    draw_save_button(f, rows[21], draft.focused == RelationshipFocus::Save);
}

fn draw_ref_prop_form(f: &mut Frame, app: &App, area: Rect) {
    let Some(draft) = app.managed.ref_prop_draft.as_ref() else {
        return;
    };
    let width = area.width.min(52);
    let height = area.height.min(13);
    let popup = Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    };
    f.render_widget(Clear, popup);
    let title = if draft.editing_index.is_some() {
        " edit relationship property "
    } else {
        " add relationship property "
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            title,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(popup);
    f.render_widget(block, popup);
    let rows = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(inner);
    draft
        .name
        .draw(f, rows[0], draft.focused == RefPropFocus::Name);
    draft
        .label
        .draw(f, rows[1], draft.focused == RefPropFocus::Label);
    draw_selector_row(
        f,
        rows[2],
        "Type",
        draft.kind.label(),
        draft.focused == RefPropFocus::Type,
    );
    draw_form_error(f, rows[3], draft.error.as_deref());
    draw_save_button(f, rows[4], draft.focused == RefPropFocus::Save);
}

fn draw_relationship_target_picker(f: &mut Frame, app: &App, area: Rect) {
    let Some(draft) = app.managed.relationship_form.as_ref() else {
        return;
    };
    let width = area.width.min(52);
    let height = area.height.clamp(6, 18);
    let popup = Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    };
    f.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            " Target object ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(popup);
    f.render_widget(block, popup);
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(inner);
    let matches = crate::managed::screen::relationship_target_matches(app);
    let count_text = format!("{} objects ", matches.len());
    crate::tui::list_chrome::draw_search_row(f, rows[0], &draft.target_query, true, &count_text);
    let selected = draft.target_selected.min(matches.len().saturating_sub(1));
    let height = rows[1].height as usize;
    let scroll = crate::tui::list_chrome::clamp_scroll(0, selected, height, matches.len());
    let lines: Vec<Line> = matches
        .iter()
        .enumerate()
        .skip(scroll)
        .take(height)
        .map(|(idx, name)| simple_pick_line(name, idx == selected))
        .collect();
    f.render_widget(Paragraph::new(lines), rows[1]);
}

fn draw_add_hook_picker(f: &mut Frame, app: &App, area: Rect) {
    let Some(draft) = app.managed.add_hook.as_ref() else {
        return;
    };
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(if draft.error.is_some() { 2 } else { 0 }),
    ])
    .split(area);
    form_title(f, rows[0], &draft.object_name, "register hook");
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "Body is edited from the script workspace",
            Style::default().fg(Color::DarkGray),
        ))),
        rows[1],
    );
    let selected = draft.selected.min(draft.events.len().saturating_sub(1));
    let height = rows[2].height as usize;
    let scroll = crate::tui::list_chrome::clamp_scroll(0, selected, height, draft.events.len());
    let lines: Vec<Line> = draft
        .events
        .iter()
        .enumerate()
        .skip(scroll)
        .take(height)
        .map(|(idx, event)| simple_pick_line(event, idx == selected))
        .collect();
    f.render_widget(Paragraph::new(lines), rows[2]);
    draw_form_error(f, rows[3], draft.error.as_deref());
}

pub fn draw_delete_field_confirm(f: &mut Frame, app: &App) {
    let field = app
        .managed
        .pending_delete
        .as_ref()
        .map(|pending| format!("{}.{}", pending.object_name, pending.field_key))
        .unwrap_or_else(|| "selected field".to_string());
    let message = format!("Delete {field}?\n\nThis can be undone from the undo log.");
    crate::tui::popup_confirm::draw(f, "Delete managed field?", &message);
}

fn draw_enum_narrow_confirm(f: &mut Frame, app: &App) {
    let removed = app
        .managed
        .editing
        .as_ref()
        .map(|edit| edit.narrowed_enum_values.join(", "))
        .unwrap_or_default();
    let message = format!(
        "Remove allowed value(s): {removed}?\n\nRecords still holding them keep reading fine and still accept patches to other properties, but a whole-record update of such a record will fail.\n\nPress y to save, n or Esc to return to the edit."
    );
    crate::tui::popup_confirm::draw(f, "Narrow allowed values?", &message);
}

fn form_title(f: &mut Frame, area: Rect, object_name: &str, action: &'static str) {
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                object_name.to_string(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("  {action}"), Style::default().fg(Color::DarkGray)),
        ])),
        area,
    );
}

fn draw_form_error(f: &mut Frame, area: Rect, error: Option<&str>) {
    if let Some(error) = error {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                error.to_string(),
                Style::default().fg(Color::Yellow),
            )))
            .wrap(Wrap { trim: false }),
            area,
        );
    }
}

fn draw_selector_row(f: &mut Frame, area: Rect, label: &str, value: &str, focused: bool) {
    let style = if focused {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(format!("{label}  "), style),
            Span::styled(value.to_string(), Style::default().fg(Color::White)),
            Span::styled("  ←/→ change", Style::default().fg(Color::DarkGray)),
        ])),
        area,
    );
}

fn draw_target_row(f: &mut Frame, area: Rect, value: &str, focused: bool) {
    let style = if focused {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Target  ", style),
            Span::styled(value.to_string(), Style::default().fg(Color::White)),
            Span::styled("  Enter pick", Style::default().fg(Color::DarkGray)),
        ])),
        area,
    );
}

fn simple_pick_line(label: &str, selected: bool) -> Line<'static> {
    let style = if selected {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    Line::from(vec![
        Span::styled(if selected { "▶ " } else { "  " }, style),
        Span::styled(label.to_string(), style),
    ])
}

fn caps_label(caps: crate::managed::state::FieldCaps) -> &'static str {
    use crate::managed::state::FieldTier;
    match caps.tier {
        FieldTier::StandardFieldOnStandardObject => "standard field",
        FieldTier::CustomFieldOnStandardObject => "custom field",
        FieldTier::FieldOnCustomObject => "custom object field",
    }
}

fn draw_save_button(f: &mut Frame, area: Rect, focused: bool) {
    if area.height == 0 {
        return;
    }
    let style = if focused {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Green).bg(Color::Indexed(234))
    };
    let row = Rect {
        x: area.x,
        y: area.y + 1,
        width: area.width,
        height: 1,
    };
    f.render_widget(Paragraph::new(Span::styled(" Save ", style)), row);
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn line_text(line: Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn property_line_shows_allowed_values_only_when_constrained() {
        let constrained = property_line(
            "status",
            &json!({"type": "string", "enum": ["new", "done"]}),
            false,
            false,
            80,
        );
        assert_eq!(line_text(constrained), "  status: string (new|done)");

        let unconstrained = property_line("status", &json!({"type": "string"}), false, false, 80);
        assert_eq!(line_text(unconstrained), "  status: string");
    }

    #[test]
    fn property_line_truncates_long_allowed_values() {
        let line = property_line(
            "status",
            &json!({"type": "string", "enum": ["new", "in_progress", "done"]}),
            false,
            false,
            23,
        );
        assert_eq!(line_text(line), "  status: string (new…)");
    }
}
