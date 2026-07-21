use ratatui::style::Color;
use serde::Deserialize;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub theme: Theme,
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

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Theme {
    pub background: Color,
    pub accent: Color,
    pub text: Color,
    pub border: Color,
    pub title: Color,
    pub danger: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            background: Color::Rgb(40, 44, 52),
            accent: Color::Rgb(176, 185, 249),
            text: Color::Rgb(153, 153, 153),
            border: Color::Rgb(148, 150, 153),
            title: Color::Rgb(218, 119, 86),
            danger: Color::Rgb(224, 108, 117),
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
    fn default_theme_preserves_the_existing_palette() {
        let theme = Theme::default();
        assert_eq!(theme.background, Color::Rgb(40, 44, 52));
        assert_eq!(theme.accent, Color::Rgb(176, 185, 249));
        assert_eq!(theme.text, Color::Rgb(153, 153, 153));
        assert_eq!(theme.border, Color::Rgb(148, 150, 153));
        assert_eq!(theme.title, Color::Rgb(218, 119, 86));
        assert_eq!(theme.danger, Color::Rgb(224, 108, 117));
    }

    #[test]
    fn partial_theme_overrides_keep_other_defaults() {
        let config: Config = toml::from_str(
            r##"
                [theme]
                accent = "#010203"
                title = "blue"
            "##,
        )
        .expect("partial theme should parse");

        assert_eq!(config.theme.accent, Color::Rgb(1, 2, 3));
        assert_eq!(config.theme.title, Color::Blue);
        assert_eq!(config.theme.text, Theme::default().text);
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
