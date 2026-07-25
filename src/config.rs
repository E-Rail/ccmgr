use ratatui::style::Color;
use serde::Deserialize;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
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

    /// The palette to draw with: the defaults, with any configured colors
    /// laid over the top.
    pub fn theme(&self) -> Theme {
        self.theme.applied_to(Theme::default())
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

/// The colors a user asked for. Everything left unset keeps its default, so
/// a config only has to name the colors it actually wants to change.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct ThemeOverrides {
    pub background: Option<Color>,
    pub accent: Option<Color>,
    pub text: Option<Color>,
    pub muted: Option<Color>,
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
            muted: self.muted.unwrap_or(base.muted),
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
    pub muted: Color,
    pub border: Color,
    pub title: Color,
    pub danger: Color,
}

/// Every default is a terminal palette entry rather than a fixed RGB value,
/// so ccmgr is drawn in the same colors as everything else in the terminal.
/// Whatever scheme the user picked - Solarized, Gruvbox, Dracula, light or
/// dark - recolors ccmgr along with it, which no hardcoded palette of ours
/// could match.
impl Default for Theme {
    fn default() -> Self {
        Self {
            // `Reset` is the terminal's own background and foreground. It
            // keeps ccmgr off any background image or transparency, and it
            // renders session titles in the same white a dark terminal uses
            // for everything else - without turning invisible the way a
            // literal `white` would on a light one.
            background: Color::Reset,
            text: Color::Reset,
            // Bright blue rather than plain blue: ANSI blue is very dark on
            // most schemes and hard to read against a dark background.
            accent: Color::LightBlue,
            title: Color::LightBlue,
            muted: Color::DarkGray,
            border: Color::DarkGray,
            danger: Color::Red,
        }
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
    fn no_default_color_is_hardcoded_to_a_fixed_value() {
        let theme = Theme::default();
        for (name, color) in [
            ("background", theme.background),
            ("accent", theme.accent),
            ("text", theme.text),
            ("border", theme.border),
            ("title", theme.title),
            ("danger", theme.danger),
        ] {
            assert!(
                !matches!(color, Color::Rgb(..) | Color::Indexed(..)),
                "{name} defaults to {color:?}, which ignores the terminal's palette"
            );
        }
    }

    #[test]
    fn the_terminals_own_colors_are_used_for_the_background_and_body_text() {
        let theme = Theme::default();
        assert_eq!(theme.background, Color::Reset);
        assert_eq!(theme.text, Color::Reset);
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

        let theme = config.theme();
        assert_eq!(theme.accent, Color::Rgb(1, 2, 3));
        assert_eq!(theme.title, Color::Blue);
        assert_eq!(theme.text, Theme::default().text);
        assert_eq!(theme.background, Color::Reset);
    }

    #[test]
    fn a_background_override_is_still_honored() {
        let config: Config = toml::from_str("[theme]\nbackground = \"#282c34\"")
            .expect("background override should parse");
        assert_eq!(config.theme().background, Color::Rgb(40, 44, 52));
    }

    #[test]
    fn palette_indexes_and_names_are_accepted() {
        let config: Config = toml::from_str("[theme]\naccent = \"5\"\ntitle = \"light-red\"")
            .expect("palette colors should parse");
        assert_eq!(config.theme().accent, Color::Indexed(5));
        assert_eq!(config.theme().title, Color::LightRed);
    }

    #[test]
    fn invalid_colors_and_unknown_fields_are_rejected() {
        assert!(toml::from_str::<Config>("[theme]\naccent = \"not-a-color\"").is_err());
        assert!(toml::from_str::<Config>("[theme]\nacccent = \"blue\"").is_err());
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
