//! Access-rule table/detail plus create, edit, and delete-confirm rendering.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Clear, Paragraph, Row, Table, Wrap},
};

use crate::access::screen::Mode;
use crate::access::state::{FormFocus, FormKind, LoadState, OptionalField, RuleMatch, RuleRow};
use crate::app::{App, InputMode};

const BODY_COLUMNS: [Constraint; 3] = [
    Constraint::Percentage(62),
    Constraint::Length(2),
    Constraint::Percentage(38),
];
const RULE_COLUMNS: [&str; 5] = ["#", "FLAGS", "PATTERN", "METHODS", "ROLES"];
const RULE_COLUMN_PERCENTAGES: [u16; RULE_COLUMNS.len()] = [6, 12, 31, 20, 31];
const RULE_COLUMN_SPACING: u16 = 1;
const DETAIL_KEYS: [&str; 6] = [
    "pattern",
    "roles",
    "methods",
    "actions",
    "customAuthz",
    "excludePatterns",
];

pub fn draw_form_modal(f: &mut Frame, app: &App, mode: Mode) {
    let Some(form) = app.access.form.as_ref() else {
        return;
    };
    f.render_widget(Clear, f.area());
    if form.confirming() {
        draw_review_modal(f, form);
        return;
    }
    let (title, default_status) = match form.kind {
        FormKind::Create { at: None } => (
            "Create access rule",
            "Append one grant to config/access".to_string(),
        ),
        // Position is presentational — rules are OR-ed — so the status says
        // where it lands without implying it matters to evaluation.
        FormKind::Create { at: Some(index) } => (
            "Create access rule",
            format!("Add one grant to config/access at #{index}"),
        ),
        FormKind::Edit { index } => (
            "Edit access rule",
            format!(
                "Rule #{index} · {} · untouched fields remain byte-identical",
                crate::access::spec::short(
                    form.original_rule_digest.as_deref().unwrap_or_default()
                )
            ),
        ),
    };
    let status = form.error.as_deref().unwrap_or(&default_status);
    let hints = if mode == Mode::Edit {
        vec![
            ("Tab", "navigate"),
            ("^S", "review"),
            ("^X/^U", "clear/keep optional"),
            ("Esc", "cancel"),
        ]
    } else {
        vec![("Tab", "navigate"), ("^S", "review"), ("Esc", "cancel")]
    };
    // Six fields + the save row are two rows each, followed by flexible space.
    const BODY: u16 = 7 * 2 + 1;
    let body = crate::tui::modal_chrome::Modal {
        title,
        status: Some(status),
        hints: &hints,
        body_height: BODY,
    }
    .draw(f, f.area());

    let rows = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(2),
        Constraint::Length(2),
        Constraint::Length(2),
        Constraint::Length(2),
        Constraint::Length(2),
        Constraint::Length(2),
        Constraint::Min(1),
    ])
    .split(body);
    form.pattern
        .draw(f, rows[0], form.focused == FormFocus::Pattern);
    form.roles
        .draw(f, rows[1], form.focused == FormFocus::Roles);
    form.methods
        .draw(f, rows[2], form.focused == FormFocus::Methods);
    draw_optional_field(
        f,
        rows[3],
        &form.actions,
        form.focused == FormFocus::Actions,
        mode,
    );
    draw_optional_field(
        f,
        rows[4],
        &form.custom_authz,
        form.focused == FormFocus::CustomAuthz,
        mode,
    );
    draw_optional_field(
        f,
        rows[5],
        &form.exclude_patterns,
        form.focused == FormFocus::ExcludePatterns,
        mode,
    );
    draw_save_button(f, rows[6], form.focused == FormFocus::Save);
}

fn draw_review_modal(f: &mut Frame, form: &crate::access::state::RuleFormState) {
    let disjunction = match form.kind {
        FormKind::Create { .. } => {
            "Rules are OR-ed: creating this rule can only grant access, never restrict it."
        }
        FormKind::Edit { .. } => {
            "Rules are OR-ed: editing or deleting a granting rule is the only way to revoke access. This can lock operators out, including you."
        }
    };
    let body_height = 9_u16
        .saturating_add(form.review_warnings.len().saturating_mul(2) as u16)
        .saturating_add(u16::from(form.role_check_note.is_some()));
    let body = crate::tui::modal_chrome::Modal {
        title: "Review config/access write",
        status: Some("Warnings are advisory; validation errors block before this step"),
        hints: &[("y", "write"), ("n/Esc", "return to form")],
        body_height,
    }
    .draw(f, f.area());

    let mut lines = vec![
        Line::from(Span::styled(
            disjunction,
            Style::default().fg(Color::Yellow),
        )),
        Line::from(""),
    ];
    if let Some(note) = &form.role_check_note {
        lines.push(Line::from(Span::styled(
            note.clone(),
            Style::default().fg(Color::Yellow),
        )));
        lines.push(Line::from(""));
    }
    if form.review_warnings.is_empty() {
        lines.push(Line::from("Validation found no warnings."));
    } else {
        lines.push(Line::from(Span::styled(
            "Validation warnings:",
            Style::default().fg(Color::Yellow),
        )));
        lines.extend(
            form.review_warnings
                .iter()
                .map(|warning| Line::from(format!("• {warning}"))),
        );
    }
    lines.extend([
        Line::from(""),
        Line::from("A mode-0600 backup will be created before the write."),
        Line::from("Write this change to the tenant?"),
    ]);
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), body);
}

fn draw_optional_field(
    f: &mut Frame,
    area: Rect,
    field: &OptionalField,
    focused: bool,
    mode: Mode,
) {
    let mut input = field.input.clone();
    if mode == Mode::Edit
        && let Some(edit) = field.edit
    {
        input.label = format!("{}  [{}]", input.label, edit.label());
    }
    input.draw(f, area, focused);
}

fn draw_save_button(f: &mut Frame, area: Rect, focused: bool) {
    let style = if focused {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Green)
    };
    f.render_widget(Paragraph::new(Span::styled(" Review & save ", style)), area);
}

pub fn draw_delete_confirm(f: &mut Frame, app: &App) {
    let Some(delete) = app.access.pending_delete.as_ref() else {
        return;
    };
    let digest = crate::access::spec::short(&delete.rule_digest);
    let message = format!(
        "Delete rule #{} ({digest})?\n\nRules are OR-ed: editing or deleting a granting rule is the only way to revoke access. This can lock operators out, including you.\n\nThe prior whole document is recorded for undo.",
        delete.index
    );
    crate::tui::popup_confirm::draw(f, "Delete access rule?", &message);
}

pub fn draw_body(f: &mut Frame, app: &App, area: Rect) {
    let Some(tenant) = app.active_tenant().map(|tenant| tenant.name.clone()) else {
        return;
    };

    match app.access.data.get(&tenant) {
        None | Some(LoadState::Loading) => {
            crate::tui::list_chrome::draw_status_line(
                f,
                area,
                "  Loading access rules...",
                Color::DarkGray,
            );
            return;
        }
        Some(LoadState::Failed(error)) => {
            crate::tui::list_chrome::draw_status_line(
                f,
                area,
                &format!("  Access rules failed: {error}"),
                Color::Red,
            );
            return;
        }
        Some(LoadState::Loaded(_)) => {}
    }

    let document = app
        .access
        .document(&tenant)
        .expect("matched Loaded access document above");
    let matches = app.access.matches(Some(&tenant));
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .split(area);
    draw_document_digest(f, &document.digest, rows[0]);

    let columns = Layout::horizontal(BODY_COLUMNS).split(rows[2]);
    draw_table(f, app, document.rows.len(), &matches, columns[0]);
    draw_detail(f, app, &matches, columns[2]);
}

fn draw_document_digest(f: &mut Frame, digest: &str, area: Rect) {
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("digest ", Style::default().fg(Color::DarkGray)),
            Span::styled(digest.to_string(), Style::default().fg(Color::Cyan)),
        ])),
        area,
    );
}

fn draw_table(f: &mut Frame, app: &App, total: usize, matches: &[RuleMatch], area: Rect) {
    let searching = app.input_mode == InputMode::Access(Mode::Search);
    let count_text = if app.access.query.is_empty() {
        format!("{total} rules ")
    } else {
        format!("{}/{} rules ", matches.len(), total)
    };
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);
    crate::tui::list_chrome::draw_search_row(f, rows[0], &app.access.query, searching, &count_text);

    let selected = app.access.selected.min(matches.len().saturating_sub(1));
    let visible_height = rows[1].height.saturating_sub(1) as usize;
    let scroll = crate::tui::list_chrome::clamp_scroll(
        app.access.scroll,
        selected,
        visible_height,
        matches.len(),
    );
    let column_widths = rendered_column_widths(rows[1].width);
    let table_rows = matches
        .iter()
        .enumerate()
        .skip(scroll)
        .take(visible_height)
        .map(|(position, item)| rule_table_row(&item.row, position == selected, column_widths))
        .collect::<Vec<_>>();
    f.render_widget(rule_table(table_rows, column_widths), rows[1]);
}

fn rule_table_row(
    row: &RuleRow,
    selected: bool,
    column_widths: [u16; RULE_COLUMNS.len()],
) -> Row<'static> {
    let style = if selected {
        Style::default()
            .fg(Color::White)
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    let summary = &row.summary;
    let values = truncate_cells(
        [
            summary.index.to_string(),
            String::new(),
            summary.pattern.clone(),
            summary.methods.clone(),
            display_roles(&summary.roles),
        ],
        column_widths,
    );
    Row::new([
        Cell::from(values[0].clone()),
        Cell::from(rule_flags(
            summary.custom_authz.is_some(),
            summary.duplicate,
        )),
        Cell::from(values[2].clone()),
        Cell::from(values[3].clone()),
        Cell::from(values[4].clone()),
    ])
    .style(style)
}

fn rule_flags(custom_authz: bool, duplicate: bool) -> Line<'static> {
    let mut spans = Vec::with_capacity(2);
    if custom_authz {
        spans.push(Span::styled("A", Style::default().fg(Color::Magenta)));
    }
    if duplicate {
        spans.push(Span::styled("D", Style::default().fg(Color::Yellow)));
    }
    Line::from(spans)
}

fn display_roles(roles: &str) -> String {
    roles
        .split(',')
        .map(|role| role.strip_prefix("internal/role/").unwrap_or(role))
        .collect::<Vec<_>>()
        .join(",")
}

fn rule_table(
    rows: impl IntoIterator<Item = Row<'static>>,
    column_widths: [u16; RULE_COLUMNS.len()],
) -> Table<'static> {
    let header = Row::new(truncate_cells(
        RULE_COLUMNS.map(str::to_string),
        column_widths,
    ))
    .style(
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    );
    Table::new(rows, rule_column_constraints())
        .header(header)
        .column_spacing(RULE_COLUMN_SPACING)
}

fn truncate_cells(
    cells: [String; RULE_COLUMNS.len()],
    column_widths: [u16; RULE_COLUMNS.len()],
) -> [String; RULE_COLUMNS.len()] {
    std::array::from_fn(|index| {
        crate::tui::list_chrome::truncate_metadata(&cells[index], usize::from(column_widths[index]))
    })
}

fn rendered_column_widths(area_width: u16) -> [u16; RULE_COLUMNS.len()] {
    Layout::horizontal(rule_column_constraints())
        .spacing(RULE_COLUMN_SPACING)
        .areas(Rect::new(0, 0, area_width, 1))
        .map(|column| column.width)
}

fn rule_column_constraints() -> [Constraint; RULE_COLUMNS.len()] {
    RULE_COLUMN_PERCENTAGES.map(Constraint::Percentage)
}

fn draw_detail(f: &mut Frame, app: &App, matches: &[RuleMatch], area: Rect) {
    let selected = app.access.selected.min(matches.len().saturating_sub(1));
    let Some(rule) = matches.get(selected).map(|item| &item.row) else {
        crate::tui::list_chrome::draw_status_line(f, area, "no matching rule", Color::DarkGray);
        return;
    };

    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                format!("rule #{}  ", rule.summary.index),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                crate::access::spec::short(&rule.summary.digest).to_string(),
                Style::default().fg(Color::Cyan),
            ),
            if rule.summary.duplicate {
                Span::styled("  duplicate", Style::default().fg(Color::Yellow))
            } else {
                Span::raw("")
            },
        ]),
        Line::from(""),
    ];
    lines.extend(
        ordered_rule_json_lines(&rule.raw)
            .flat_map(|line| crate::tui::list_chrome::wrap_lines(&line, area.width))
            .map(Line::from),
    );
    let rendered_height = lines.len();
    let scroll = app
        .access
        .detail_scroll
        .clamp(rendered_height, usize::from(area.height));
    f.render_widget(
        Paragraph::new(lines)
            .style(Style::default().fg(Color::White))
            .scroll((scroll as u16, 0)),
        area,
    );
}

fn ordered_rule_json_lines(rule: &serde_json::Value) -> impl Iterator<Item = String> {
    // `serde_json::Value` and its string object keys have no fallible custom
    // serializer, so these expects are invariant checks rather than live error
    // paths. Avoiding a dead fallback keeps the render path simple without
    // accepting a panic risk that could strand the terminal in raw mode.
    let Some(object) = rule.as_object() else {
        return serde_json::to_string_pretty(rule)
            .expect("serialize serde_json::Value")
            .lines()
            .map(str::to_string)
            .collect::<Vec<_>>()
            .into_iter();
    };

    let mut keys = DETAIL_KEYS
        .into_iter()
        .filter(|key| object.contains_key(*key))
        .collect::<Vec<_>>();
    let mut remaining = object
        .keys()
        .map(String::as_str)
        .filter(|key| !DETAIL_KEYS.contains(key))
        .collect::<Vec<_>>();
    remaining.sort_unstable();
    keys.extend(remaining);

    let mut lines = vec!["{".to_string()];
    let key_count = keys.len();
    for (index, key) in keys.into_iter().enumerate() {
        let encoded_key = serde_json::to_string(key).expect("serialize JSON object key");
        let value =
            serde_json::to_string_pretty(&object[key]).expect("serialize serde_json::Value");
        let mut value_lines = value.lines();
        let first = value_lines.next().unwrap_or_default();
        lines.push(format!("  {encoded_key}: {first}"));
        lines.extend(value_lines.map(|line| format!("  {line}")));
        if index + 1 < key_count
            && let Some(last) = lines.last_mut()
        {
            last.push(',');
        }
    }
    lines.push("}".to_string());
    lines.into_iter()
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend};
    use serde_json::json;

    use crate::access::spec::RuleSummary;

    use super::*;

    const INDEX_MIN_CONTENT_WIDTH: u16 = 2;
    const FLAGS_HEADER_MIN_CONTENT_WIDTH: u16 = 5;

    fn render_test_table(width: u16) -> Vec<String> {
        let backend = TestBackend::new(width, 2);
        let mut terminal = Terminal::new(backend).unwrap();
        let row = RuleRow {
            summary: RuleSummary {
                index: 65,
                digest: "01234567".into(),
                duplicate: true,
                pattern: "pattern-segment-".repeat(20),
                methods: "M".into(),
                roles: "internal/role/x,*".into(),
                actions: None,
                custom_authz: Some("allow()".into()),
                exclude_patterns: None,
            },
            raw: json!({}),
        };
        terminal
            .draw(|frame| {
                let [table_area, _, _] = Layout::horizontal(BODY_COLUMNS).areas(frame.area());
                let widths = rendered_column_widths(table_area.width);
                let table_row = rule_table_row(&row, false, widths);
                frame.render_widget(rule_table([table_row], widths), table_area);
            })
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .chunks(usize::from(width))
            .map(|cells| cells.iter().map(|cell| cell.symbol()).collect())
            .collect()
    }

    #[test]
    fn rule_column_constraints_are_percentages_totalling_one_hundred() {
        // Changing the one percentage array without balancing it makes this
        // structural guard fail.
        assert_eq!(RULE_COLUMN_PERCENTAGES.into_iter().sum::<u16>(), 100);
    }

    #[test]
    fn narrow_table_preserves_required_index_and_flag_content() {
        // Squeezing either leading percentage below its actual content need
        // makes two-digit indices or the FLAGS header clip at the minimum
        // supported width.
        let [table_area, _, _] = Layout::horizontal(BODY_COLUMNS).areas(Rect::new(
            0,
            0,
            crate::tui::MIN_TERMINAL_WIDTH,
            1,
        ));
        let widths = rendered_column_widths(table_area.width);
        assert!(
            widths[0] >= INDEX_MIN_CONTENT_WIDTH,
            "index width {widths:?}"
        );
        assert!(
            widths[1] >= FLAGS_HEADER_MIN_CONTENT_WIDTH,
            "flags width {widths:?}"
        );
    }

    #[test]
    fn minimum_width_table_layout_is_pinned() {
        // Pinned at the supported floor, which is where the percentages are
        // tightest. Moving the floor is expected to fail here and be re-pinned.
        let lines = render_test_table(crate::tui::MIN_TERMINAL_WIDTH);
        assert_eq!(
            lines[0].trim_end(),
            "#   FLAGS  PATTERN     METHODS   ROLES"
        );
        assert_eq!(lines[1].trim_end(), "65  AD     pattern-se… M         x,*");
    }

    #[test]
    fn rendered_table_keeps_the_ellipsis_at_the_column_edge() {
        // Desynchronising our width calculation from Ratatui's actual Table
        // layout removes the exact "ellipsis, spacing, M" boundary.
        for width in [crate::tui::MIN_TERMINAL_WIDTH, 120] {
            let lines = render_test_table(width);
            assert!(lines[0].contains("#"), "{width}: {:?}", lines[0]);
            assert!(lines[0].contains("FLAGS"), "{width}: {:?}", lines[0]);
            assert!(!lines[0].contains("DIGEST"), "{width}: {:?}", lines[0]);
            assert!(lines[1].contains("65"), "{width}: {:?}", lines[1]);
            assert!(lines[1].contains("AD"), "{width}: {:?}", lines[1]);
            assert!(lines[1].contains("… M"), "{width}: {:?}", lines[1]);
        }
    }

    #[test]
    fn tui_roles_strip_only_the_exact_internal_role_prefix() {
        // Moving this transform into RuleSummary changes CLI values; broad
        // trimming would hide malformed entries that the operator must see.
        assert_eq!(display_roles("internal/role/x,*"), "x,*");
        assert_eq!(
            display_roles(" internal/role/x,external/role/y"),
            " internal/role/x,external/role/y"
        );
    }

    #[test]
    fn flags_use_semantic_custom_authz_and_duplicate_colours() {
        // Collapsing flags back to an uncoloured marker string loses the
        // semantic distinction between script gating and duplicate caution.
        let flags = rule_flags(true, true);
        assert_eq!(flags.spans[0].content.as_ref(), "A");
        assert_eq!(flags.spans[0].style.fg, Some(Color::Magenta));
        assert_eq!(flags.spans[1].content.as_ref(), "D");
        assert_eq!(flags.spans[1].style.fg, Some(Color::Yellow));
    }

    #[test]
    fn detail_orders_known_fields_first_without_dropping_unknown_keys() {
        // Returning to serde_json's alphabetical object rendering moves roles
        // after methods; projecting RuleView drops the unknown key.
        let lines = ordered_rule_json_lines(&json!({
            "unknown": {"preserve": true},
            "roles": "internal/role/x",
            "pattern": "managed/x",
            "methods": "read",
            "customAuthz": "allow()"
        }))
        .collect::<Vec<_>>()
        .join("\n");

        let pattern = lines.find("\"pattern\"").unwrap();
        let roles = lines.find("\"roles\"").unwrap();
        let methods = lines.find("\"methods\"").unwrap();
        let custom_authz = lines.find("\"customAuthz\"").unwrap();
        let unknown = lines.find("\"unknown\"").unwrap();
        assert!(pattern < roles && roles < methods && methods < custom_authz);
        assert!(custom_authz < unknown);
        assert!(lines.contains("\"preserve\": true"));
    }
}
