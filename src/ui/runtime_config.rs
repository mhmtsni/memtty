use winit::window::Theme;

pub struct AppRuntimeConfig {
    pub window_title: String,
    pub theme: Option<Theme>,
    pub font_size: f32,
}

impl AppRuntimeConfig {
    pub fn from_env() -> Self {
        Self {
            window_title: std::env::var("TERMINAL_TITLE")
                .unwrap_or_else(|_| "terminal".to_string()),
            theme: match std::env::var("TERMINAL_THEME") {
                Ok(v) if v.eq_ignore_ascii_case("light") => Some(Theme::Light),
                Ok(v) if v.eq_ignore_ascii_case("dark") => Some(Theme::Dark),
                _ => Some(Theme::Dark),
            },
            font_size: std::env::var("TERMINAL_FONT_SIZE")
                .ok()
                .and_then(|v| v.parse::<f32>().ok())
                .filter(|v| *v >= 6.0 && *v <= 72.0)
                .unwrap_or(25.0),
        }
    }
}
