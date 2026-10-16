use ratatui::style::{Color, Modifier, Style};

/// Neutral dark surfaces with a magenta accent, in the spirit of GrokNight.
#[derive(Clone, Copy)]
pub struct Theme {
    pub bg: Color,
    pub bg_raised: Color,
    pub bg_selected: Color,
    pub bg_field: Color,
    pub border: Color,
    pub text: Color,
    pub dim: Color,
    pub faint: Color,
    pub accent: Color,
    pub ok: Color,
    pub warn: Color,
    pub danger: Color,
    pub p1: Color,
    pub p2: Color,
    pub p3: Color,
}

impl Theme {
    pub fn grok_night() -> Self {
        Self {
            bg: Color::Rgb(22, 22, 24),
            bg_raised: Color::Rgb(30, 30, 33),
            bg_selected: Color::Rgb(48, 32, 42),
            bg_field: Color::Rgb(34, 34, 38),
            border: Color::Rgb(58, 56, 60),
            text: Color::Rgb(232, 230, 226),
            dim: Color::Rgb(154, 150, 144),
            faint: Color::Rgb(108, 104, 99),
            accent: Color::Rgb(226, 120, 166),
            ok: Color::Rgb(126, 184, 146),
            warn: Color::Rgb(214, 176, 108),
            danger: Color::Rgb(216, 112, 118),
            p1: Color::Rgb(219, 76, 63),
            p2: Color::Rgb(235, 148, 62),
            p3: Color::Rgb(96, 154, 220),
        }
    }

    pub fn fg(self, color: Color) -> Style {
        Style::new().fg(color)
    }

    pub fn text(self) -> Style {
        self.fg(self.text)
    }

    pub fn dim(self) -> Style {
        self.fg(self.dim)
    }

    pub fn faint(self) -> Style {
        self.fg(self.faint)
    }

    pub fn accent(self) -> Style {
        self.fg(self.accent)
    }

    pub fn bold_accent(self) -> Style {
        self.accent().add_modifier(Modifier::BOLD)
    }

    pub fn bold(self) -> Style {
        self.text().add_modifier(Modifier::BOLD)
    }
}

/// Todoist project color names, tuned so they read on the dark surface.
pub fn project_color(name: &str) -> Color {
    match name {
        "berry_red" => Color::Rgb(184, 37, 111),
        "red" => Color::Rgb(219, 64, 53),
        "orange" => Color::Rgb(255, 153, 51),
        "yellow" => Color::Rgb(230, 190, 60),
        "olive_green" => Color::Rgb(175, 184, 59),
        "lime_green" => Color::Rgb(126, 204, 73),
        "green" => Color::Rgb(70, 170, 90),
        "mint_green" => Color::Rgb(106, 204, 188),
        "teal" => Color::Rgb(50, 170, 190),
        "sky_blue" => Color::Rgb(70, 180, 230),
        "light_blue" => Color::Rgb(150, 195, 235),
        "blue" => Color::Rgb(110, 145, 245),
        "grape" => Color::Rgb(160, 120, 245),
        "violet" => Color::Rgb(190, 120, 235),
        "lavender" => Color::Rgb(220, 160, 220),
        "magenta" => Color::Rgb(224, 81, 148),
        "salmon" => Color::Rgb(255, 141, 133),
        "charcoal" => Color::Rgb(150, 150, 150),
        "grey" | "gray" => Color::Rgb(168, 164, 158),
        "taupe" => Color::Rgb(204, 172, 147),
        _ => Color::Rgb(154, 150, 144),
    }
}
