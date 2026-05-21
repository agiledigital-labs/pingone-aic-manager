use ratatui::style::Color;

use crate::config::tenant::TenantTheme;

/// Visual style for a tenant's environment chip — color pair + glyph + label.
/// Looked up via `style_for(theme)`.
pub struct ThemeStyle {
    pub fg: Color,
    pub bg: Color,
    pub glyph: &'static str,
    pub label: &'static str,
}

pub fn style_for(theme: TenantTheme) -> ThemeStyle {
    match theme {
        TenantTheme::Sandbox => ThemeStyle {
            fg: Color::Black,
            bg: Color::Green,
            glyph: "▪",
            label: "sandbox",
        },
        TenantTheme::Development => ThemeStyle {
            fg: Color::Black,
            bg: Color::Blue,
            glyph: "▪",
            label: "development",
        },
        TenantTheme::Staging => ThemeStyle {
            fg: Color::Black,
            bg: Color::Yellow,
            glyph: "▪",
            label: "staging",
        },
        TenantTheme::Production => ThemeStyle {
            fg: Color::White,
            bg: Color::Red,
            glyph: "⚠",
            label: "production",
        },
    }
}
