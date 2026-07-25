use ratatui::style::Color;
use serde::Deserialize;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub appearance: AppearanceSetting,
    pub theme: ThemeOverrides,
}

impl Config {
    pub fn load() -> io::Result<Self> {
        match Self::path() {
            Some(path) => Self::load_from_path(&path),
            None => Ok(Self::default()),
        }
    }

    pub fn path() -> Option<PathBuf> {
        std::env::var_os("CCMGR_CONFIG")
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                dirs::config_dir().map(|directory| directory.join("ccmgr").join("config.toml"))
            })
    }

    /// Pick the palette to draw with. `detected` is what the terminal
    /// reported about its own background; an explicit `appearance` in the
    /// config overrides it, and individual `[theme]` colors override that.
    pub fn theme(&self, detected: Appearance) -> Theme {
        let appearance = match self.appearance {
            AppearanceSetting::Auto => detected,
            AppearanceSetting::Dark => Appearance::Dark,
            AppearanceSetting::Light => Appearance::Light,
        };
        self.theme.applied_to(Theme::for_appearance(appearance))
    }

    fn load_from_path(path: &Path) -> io::Result<Self> {
        let source = match fs::read_to_string(path) {
            Ok(source) => source,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => {
                return Err(io::Error::new(
                    error.kind(),
                    format!("failed to read config {}: {error}", path.display()),
                ));
            }
        };

        toml::from_str(&source).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("failed to parse config {}: {error}", path.display()),
            )
        })
    }
}

/// Whether the terminal is painting light text on a dark background or the
/// other way around. Detected at startup - see `crate::appearance`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Appearance {
    Dark,
    Light,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AppearanceSetting {
    #[default]
    Auto,
    Dark,
    Light,
}

/// The colors a user asked for. Everything left unset falls back to the
/// palette for the detected appearance, so a config only has to name the
/// colors it actually wants to change.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct ThemeOverrides {
    pub background: Option<Color>,
    pub accent: Option<Color>,
    pub text: Option<Color>,
    pub border: Option<Color>,
    pub title: Option<Color>,
    pub danger: Option<Color>,
}

impl ThemeOverrides {
    fn applied_to(&self, base: Theme) -> Theme {
        Theme {
            background: self.background.unwrap_or(base.background),
            accent: self.accent.unwrap_or(base.accent),
            text: self.text.unwrap_or(base.text),
            border: self.border.unwrap_or(base.border),
            title: self.title.unwrap_or(base.title),
            danger: self.danger.unwrap_or(base.danger),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    pub background: Color,
    pub accent: Color,
    pub text: Color,
    pub border: Color,
    pub title: Color,
    pub danger: Color,
}

impl Theme {
    pub fn for_appearance(appearance: Appearance) -> Self {
        match appearance {
            Appearance::Dark => Self::dark(),
            Appearance::Light => Self::light(),
        }
    }

    /// For light text on a dark terminal.
    pub fn dark() -> Self {
        Self {
            // Never paint our own background - `Reset` leaves the terminal's
            // own background showing through, so ccmgr sits on whatever the
            // user's color scheme (or transparency) already draws.
            background: Color::Reset,
            accent: Color::Rgb(176, 185, 249),
            text: Color::Rgb(153, 153, 153),
            border: Color::Rgb(148, 150, 153),
            title: Color::Rgb(218, 119, 86),
            danger: Color::Rgb(224, 108, 117),
        }
    }

    /// The same palette darkened until it stays readable on a light
    /// terminal - the dark one's pale accent all but vanishes on white.
    pub fn light() -> Self {
        Self {
            background: Color::Reset,
            accent: Color::Rgb(74, 88, 199),
            text: Color::Rgb(90, 90, 90),
            border: Color::Rgb(122, 124, 128),
            title: Color::Rgb(191, 87, 51),
            danger: Color::Rgb(190, 52, 58),
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_TEMP_PATH: AtomicUsize = AtomicUsize::new(0);

    fn temp_path(name: &str) -> PathBuf {
        let sequence = NEXT_TEMP_PATH.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "ccmgr-config-test-{}-{sequence}-{name}",
            std::process::id()
        ))
    }

    #[test]
    fn neither_palette_paints_a_background() {
        assert_eq!(Theme::dark().background, Color::Reset);
        assert_eq!(Theme::light().background, Color::Reset);
    }

    #[test]
    fn the_dark_palette_preserves_the_existing_colors() {
        let theme = Theme::dark();
        assert_eq!(theme.accent, Color::Rgb(176, 185, 249));
        assert_eq!(theme.text, Color::Rgb(153, 153, 153));
        assert_eq!(theme.border, Color::Rgb(148, 150, 153));
        assert_eq!(theme.title, Color::Rgb(218, 119, 86));
        assert_eq!(theme.danger, Color::Rgb(224, 108, 117));
    }

    #[test]
    fn every_light_palette_color_stays_readable_on_white() {
        // WCAG 2.1 contrast against white. Borders are decorative, so they
        // only need the 3:1 required of UI components; everything else
        // carries text and has to clear 4.5:1.
        let theme = Theme::light();
        for (name, color, minimum) in [
            ("accent", theme.accent, 4.5),
            ("text", theme.text, 4.5),
            ("border", theme.border, 3.0),
            ("title", theme.title, 4.5),
            ("danger", theme.danger, 4.5),
        ] {
            let Color::Rgb(red, green, blue) = color else {
                panic!("{name} should be an explicit RGB color");
            };
            let ratio = 1.05 / (relative_luminance(red, green, blue) + 0.05);
            assert!(
                ratio >= minimum,
                "{name} only reaches {ratio:.2}:1 on white, needs {minimum}:1"
            );
        }
    }

    fn relative_luminance(red: u8, green: u8, blue: u8) -> f64 {
        fn channel(value: u8) -> f64 {
            let value = value as f64 / 255.0;
            if value <= 0.03928 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * channel(red) + 0.7152 * channel(green) + 0.0722 * channel(blue)
    }

    #[test]
    fn auto_appearance_follows_the_terminal() {
        let config = Config::default();
        assert_eq!(config.theme(Appearance::Dark), Theme::dark());
        assert_eq!(config.theme(Appearance::Light), Theme::light());
    }

    #[test]
    fn an_explicit_appearance_overrides_what_the_terminal_reports() {
        let config: Config =
            toml::from_str("appearance = \"dark\"").expect("appearance should parse");
        assert_eq!(config.theme(Appearance::Light), Theme::dark());

        let config: Config =
            toml::from_str("appearance = \"light\"").expect("appearance should parse");
        assert_eq!(config.theme(Appearance::Dark), Theme::light());
    }

    #[test]
    fn partial_theme_overrides_keep_the_rest_of_the_palette() {
        let config: Config = toml::from_str(
            r##"
                [theme]
                accent = "#010203"
                title = "blue"
            "##,
        )
        .expect("partial theme should parse");

        let theme = config.theme(Appearance::Light);
        assert_eq!(theme.accent, Color::Rgb(1, 2, 3));
        assert_eq!(theme.title, Color::Blue);
        assert_eq!(theme.text, Theme::light().text);
        assert_eq!(theme.background, Color::Reset);
    }

    #[test]
    fn a_background_override_is_still_honored() {
        let config: Config = toml::from_str("[theme]\nbackground = \"#282c34\"")
            .expect("background override should parse");
        assert_eq!(
            config.theme(Appearance::Dark).background,
            Color::Rgb(40, 44, 52)
        );
    }

    #[test]
    fn invalid_colors_and_unknown_fields_are_rejected() {
        assert!(toml::from_str::<Config>("[theme]\naccent = \"not-a-color\"").is_err());
        assert!(toml::from_str::<Config>("[theme]\nacccent = \"blue\"").is_err());
        assert!(toml::from_str::<Config>("appearance = \"sepia\"").is_err());
    }

    #[test]
    fn a_missing_config_file_uses_defaults() {
        let path = temp_path("missing.toml");
        let config = Config::load_from_path(&path).expect("missing config should be optional");
        assert_eq!(config, Config::default());
    }

    #[test]
    fn malformed_config_errors_include_the_path() {
        let path = temp_path("malformed.toml");
        fs::write(&path, "[theme\naccent = \"blue\"").expect("fixture should be written");

        let error = Config::load_from_path(&path).expect_err("invalid TOML should fail");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains(&path.display().to_string()));

        fs::remove_file(path).expect("fixture should be removed");
    }
}
