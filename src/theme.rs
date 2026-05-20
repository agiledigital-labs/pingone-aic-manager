use ratatui::style::Color;

use crate::config::tenant::TenantTheme;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Theme {
    Sandbox,
    Dev,
    Staging,
    Prod,
}

pub struct ThemeStyle {
    pub fg: Color,
    pub bg: Color,
    pub glyph: &'static str,
    pub label: &'static str,
}

impl Theme {
    pub fn style(self) -> ThemeStyle {
        match self {
            Theme::Sandbox => ThemeStyle { fg: Color::Black, bg: Color::Green,  glyph: "▪", label: "sandbox" },
            Theme::Dev     => ThemeStyle { fg: Color::Black, bg: Color::Blue,   glyph: "▪", label: "dev"     },
            Theme::Staging => ThemeStyle { fg: Color::Black, bg: Color::Yellow, glyph: "▪", label: "staging" },
            Theme::Prod    => ThemeStyle { fg: Color::White, bg: Color::Red,    glyph: "⚠", label: "prod"    },
        }
    }

    pub fn from_tenant(t: TenantTheme) -> Self {
        match t {
            TenantTheme::Sandbox => Theme::Sandbox,
            TenantTheme::Dev     => Theme::Dev,
            TenantTheme::Staging => Theme::Staging,
            TenantTheme::Prod    => Theme::Prod,
        }
    }
}
