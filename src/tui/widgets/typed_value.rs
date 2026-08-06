//! Shape-aware value input shared by feature forms.
//!
//! The inner [`TextField`] is the sole storage for every shape, including
//! booleans. Changing shape therefore changes only editing and rendering; it
//! never discards text that the user may want to recover by changing back.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use super::TextField;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonShape {
    Any,
    Object,
    Array,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueShape {
    Text,
    MultilineText,
    Bool,
    Integer,
    Decimal,
    Json(JsonShape),
}

impl ValueShape {
    pub fn validate(self, value: &str) -> Result<(), String> {
        let value = value.trim();
        match self {
            Self::Text | Self::MultilineText => Ok(()),
            Self::Bool => match value {
                "true" | "false" => Ok(()),
                _ => Err("Value must be 'true' or 'false'".into()),
            },
            Self::Integer => value
                .parse::<i64>()
                .map(|_| ())
                .map_err(|_| "Value must be an integer".into()),
            Self::Decimal => value
                .parse::<f64>()
                .map(|_| ())
                .map_err(|_| "Value must be a number".into()),
            Self::Json(shape) => match serde_json::from_str::<serde_json::Value>(value) {
                Ok(serde_json::Value::Object(_))
                    if matches!(shape, JsonShape::Any | JsonShape::Object) =>
                {
                    Ok(())
                }
                Ok(serde_json::Value::Array(_))
                    if matches!(shape, JsonShape::Any | JsonShape::Array) =>
                {
                    Ok(())
                }
                Ok(_) if shape == JsonShape::Any => Ok(()),
                Ok(_) if shape == JsonShape::Object => {
                    Err("Value must be a JSON object (e.g. {\"k\":\"v\"})".into())
                }
                Ok(_) => Err("Value must be a JSON array (e.g. [1,2,3])".into()),
                Err(error) => Err(format!("Value must be valid JSON: {error}")),
            },
        }
    }

    pub fn hint(self) -> TypedValueHint {
        match self {
            Self::Bool => TypedValueHint::Toggle,
            Self::MultilineText => TypedValueHint::InsertNewline,
            _ => TypedValueHint::Advance,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypedValueHint {
    Toggle,
    InsertNewline,
    Advance,
}

#[derive(Debug, Clone)]
pub struct TypedValueField {
    shape: ValueShape,
    required: bool,
    field: TextField,
    error: Option<String>,
}

impl TypedValueField {
    pub fn new(label: impl Into<String>, shape: ValueShape, required: bool) -> Self {
        let label = label.into();
        let field = text_field(&label, shape);
        let mut value = Self {
            shape,
            required,
            field,
            error: None,
        };
        value.revalidate();
        value
    }

    pub fn with_initial(mut self, value: impl Into<String>) -> Self {
        self.field.set(value);
        self.revalidate();
        self
    }

    pub fn value(&self) -> &str {
        &self.field.value
    }

    /// Replace the text wholesale. Deliberately the *only* way to write the
    /// value other than [`Self::handle_key`]: every mutation has to re-run
    /// validation, or the inline error goes stale and the form shows a
    /// complaint about text the user has already fixed.
    pub fn set(&mut self, value: impl Into<String>) {
        self.field.set(value);
        self.revalidate();
    }

    pub fn trimmed(&self) -> &str {
        self.field.trimmed()
    }

    pub fn is_empty(&self) -> bool {
        self.field.is_empty()
    }

    pub fn shape(&self) -> ValueShape {
        self.shape
    }

    pub fn set_shape(&mut self, shape: ValueShape) {
        self.shape = shape;
        self.field.kind = text_field(&self.field.label, shape).kind;
        self.revalidate();
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> bool {
        let accepted = match self.shape {
            ValueShape::Bool => match key.code {
                KeyCode::Char(' ') | KeyCode::Enter => {
                    self.cycle_bool();
                    true
                }
                _ => false,
            },
            ValueShape::MultilineText if key.code == KeyCode::Enter => {
                self.field.push_newline();
                true
            }
            _ => self.field.handle_key(key),
        };
        if accepted {
            self.revalidate();
        }
        accepted
    }

    pub fn validate(&self) -> Result<(), String> {
        if !self.required && self.field.is_empty() {
            return Ok(());
        }
        if self.required && self.field.value.is_empty() {
            return match self.shape.validate("") {
                Ok(()) => Err("Value cannot be empty".into()),
                error => error,
            };
        }
        self.shape.validate(&self.field.value)
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn draw(&self, frame: &mut Frame, area: Rect, focused: bool) {
        if self.shape != ValueShape::Bool {
            self.field.draw(frame, area, focused);
            return;
        }
        if area.height == 0 {
            return;
        }
        let label_style = if focused {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        frame.render_widget(
            Paragraph::new(Span::styled(self.field.label.clone(), label_style)),
            Rect { height: 1, ..area },
        );
        if area.height > 1 {
            let value = match self.field.value.as_str() {
                "true" => Some(true),
                "false" => Some(false),
                _ => None,
            };
            let label = match value {
                Some(true) => "true",
                Some(false) => "false",
                None => "(not set)",
            };
            draw_bool_row(
                frame,
                Rect {
                    y: area.y + 1,
                    height: 1,
                    ..area
                },
                label,
                value,
                focused,
                true,
            );
        }
    }

    pub fn height_hint(&self) -> u16 {
        match self.shape {
            ValueShape::MultilineText => 5,
            _ => 2,
        }
    }

    pub fn hint(&self) -> TypedValueHint {
        self.shape.hint()
    }

    fn cycle_bool(&mut self) {
        let next = if self.required {
            match self.field.value.as_str() {
                "true" => "false",
                _ => "true",
            }
        } else {
            match self.field.value.as_str() {
                "" => "true",
                "true" => "false",
                _ => "",
            }
        };
        self.field.set(next);
    }

    fn revalidate(&mut self) {
        self.error = self.validate().err();
    }
}

fn text_field(label: &str, shape: ValueShape) -> TextField {
    if shape == ValueShape::MultilineText {
        TextField::textarea(label)
    } else {
        TextField::single_line(label)
    }
}

/// Shared boolean row. `None` is rendered as the optional, unset state.
/// Passing `Some` preserves the managed-form rendering byte-for-byte.
pub fn draw_bool_row(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    value: Option<bool>,
    focused: bool,
    enabled: bool,
) {
    if area.height == 0 {
        return;
    }
    let foreground = match (enabled, focused) {
        (false, _) => Color::DarkGray,
        (true, true) => Color::Yellow,
        (true, false) => Color::Gray,
    };
    let style = if focused {
        Style::default().fg(foreground).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(foreground)
    };
    let mark = match value {
        Some(true) => "x",
        Some(false) => " ",
        None => "-",
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(format!("[{mark}] "), style),
            Span::styled(label.to_string(), style),
            Span::styled("  Space/Enter toggle", Style::default().fg(Color::DarkGray)),
        ])),
        area,
    );
}

#[cfg(test)]
mod tests {
    use crossterm::event::KeyModifiers;
    use ratatui::{Terminal, backend::TestBackend};

    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::empty())
    }

    #[test]
    fn validates_every_shape() {
        assert!(ValueShape::Text.validate("anything").is_ok());
        assert!(ValueShape::MultilineText.validate("a\nb").is_ok());
        assert!(ValueShape::Bool.validate("true").is_ok());
        assert!(ValueShape::Bool.validate("yes").is_err());
        assert!(ValueShape::Integer.validate("-3").is_ok());
        assert!(ValueShape::Integer.validate("1.5").is_err());
        assert!(ValueShape::Decimal.validate("1.0").is_ok());
        assert!(ValueShape::Decimal.validate("-3.00").is_ok());
        assert!(
            ValueShape::Json(JsonShape::Object)
                .validate(r#"{"a":1}"#)
                .is_ok()
        );
        assert!(
            ValueShape::Json(JsonShape::Object)
                .validate("[1,2]")
                .is_err()
        );
        assert!(ValueShape::Json(JsonShape::Array).validate("[1,2]").is_ok());
        assert!(
            ValueShape::Json(JsonShape::Array)
                .validate(r#"{"a":1}"#)
                .is_err()
        );
        assert!(ValueShape::Json(JsonShape::Any).validate("null").is_ok());
    }

    #[test]
    fn bool_cycles_required_and_optional_states() {
        let mut required = TypedValueField::new("Value", ValueShape::Bool, true);
        required.handle_key(&key(KeyCode::Enter));
        assert_eq!(required.value(), "true");
        required.handle_key(&key(KeyCode::Char(' ')));
        assert_eq!(required.value(), "false");
        required.handle_key(&key(KeyCode::Enter));
        assert_eq!(required.value(), "true");

        let mut optional = TypedValueField::new("Value", ValueShape::Bool, false);
        for expected in ["true", "false", ""] {
            optional.handle_key(&key(KeyCode::Enter));
            assert_eq!(optional.value(), expected);
        }
    }

    #[test]
    fn changing_shape_preserves_and_revalidates_text() {
        let mut field = TypedValueField::new("Value", ValueShape::Text, true).with_initial("true");
        field.set_shape(ValueShape::Bool);
        assert_eq!(field.value(), "true");
        assert_eq!(field.error(), None);

        field.set_shape(ValueShape::Text);
        field.set("yes");
        field.set_shape(ValueShape::Bool);
        assert_eq!(field.value(), "yes");
        assert!(field.error().is_some());
    }

    #[test]
    fn optional_empty_is_valid_but_required_empty_uses_shape_error() {
        assert!(
            TypedValueField::new("Value", ValueShape::Integer, false)
                .validate()
                .is_ok()
        );
        assert_eq!(
            TypedValueField::new("Value", ValueShape::Integer, true)
                .validate()
                .unwrap_err(),
            "Value must be an integer"
        );
    }

    #[test]
    fn renders_all_boolean_states() {
        for (value, expected) in [
            ("true", "[x] true"),
            ("false", "[ ] false"),
            ("", "[-] (not set)"),
        ] {
            let backend = TestBackend::new(40, 2);
            let mut terminal = Terminal::new(backend).unwrap();
            let field = TypedValueField::new("Value", ValueShape::Bool, false).with_initial(value);
            terminal
                .draw(|frame| field.draw(frame, frame.area(), false))
                .unwrap();
            let rendered = terminal.backend().buffer().content[40..80]
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();
            assert!(rendered.starts_with(expected), "{rendered:?}");
        }
    }
}
