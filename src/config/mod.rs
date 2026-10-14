//! User configuration: the `Config` struct, defaults, and load/save of `config.toml`.
//! Kept separate from the UI so it can be unit-tested and later hot-reloaded.

pub mod theme;

use std::path::{Path, PathBuf};
use std::{fs, io};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{error, info, warn};

use theme::Theme;

/// Font family of the bundled font, used when `font.path` is not set.
pub const BUNDLED_FONT_FAMILY: &str = "JetBrains Mono";

/// Comment block written above the defaults in a freshly created config file. It lists
/// the optional keys, which would otherwise be invisible because unset options aren't
/// serialized.
const CONFIG_HEADER: &str = "\
# jterm configuration. Remove a key to use its default.
#
# Optional keys not shown below:
#   [font]  path = \"/path/to/font.ttf\"   # load this font file instead of the bundled one
#   [shell] program = \"/bin/zsh\"         # default: detected from the environment

";

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("no home directory found, so the config file location is unknown")]
    NoConfigDir,
    #[error("failed to read {}: {source}", path.display())]
    Read { path: PathBuf, source: io::Error },
    #[error("failed to write {}: {source}", path.display())]
    Write { path: PathBuf, source: io::Error },
    #[error("invalid config: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("invalid config: {0}")]
    Invalid(String),
}

/// Font sizes outside this range are almost certainly typos, and extreme values can
/// produce absurd grids or exhaust egui's font atlas.
const FONT_SIZE_RANGE: std::ops::RangeInclusive<f32> = 4.0..=200.0;

/// All user settings. Every section and key is optional in the file; anything missing
/// takes its default, so a config only needs to contain what the user changed.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub font: FontConfig,
    pub shell: ShellConfig,
    pub scrollback: ScrollbackConfig,
    pub cursor: CursorConfig,
    pub window: WindowConfig,
    pub bell: BellConfig,
    pub theme: Theme,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FontConfig {
    /// Only the bundled family is available by name for now. Other fonts are loaded
    /// with `path`, since egui can't look up system fonts by family name.
    pub family: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    /// Size in points (logical pixels).
    pub size: f32,
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            family: BUNDLED_FONT_FAMILY.to_owned(),
            path: None,
            size: 14.0,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShellConfig {
    /// `None` means "detect from the environment" when the session is spawned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub program: Option<String>,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScrollbackConfig {
    pub lines: usize,
}

impl Default for ScrollbackConfig {
    fn default() -> Self {
        Self { lines: 10_000 }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CursorStyle {
    #[default]
    Block,
    Beam,
    Underline,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CursorConfig {
    pub style: CursorStyle,
    /// Off by default: blinking needs a repaint timer, and an idle terminal should use no CPU.
    pub blink: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowConfig {
    /// Space between the window edge and the terminal grid, in points.
    pub padding: f32,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self { padding: 8.0 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BellConfig {
    /// Flash the window on BEL.
    pub visual: bool,
}

impl Default for BellConfig {
    fn default() -> Self {
        Self { visual: true }
    }
}

impl Config {
    /// Location of the config file: `~/.config/jterm/config.toml` on Linux and the
    /// platform's standard config directory elsewhere.
    pub fn path() -> Result<PathBuf, ConfigError> {
        ProjectDirs::from("", "", "jterm")
            .map(|dirs| dirs.config_dir().join("config.toml"))
            .ok_or(ConfigError::NoConfigDir)
    }

    /// Loads the user's config. Never fails: every problem is logged and the defaults
    /// are used, so a broken config file can't stop the terminal from opening.
    pub fn load() -> Self {
        match Self::path() {
            Ok(path) => Self::load_from(&path),
            Err(e) => {
                warn!("{e}; using the default config");
                Self::default()
            }
        }
    }

    /// Loads the config at `path`, writing the defaults there first if the file doesn't exist.
    pub fn load_from(path: &Path) -> Self {
        match Self::read(path) {
            Ok(config) => {
                info!(path = %path.display(), "loaded config");
                config
            }
            Err(ConfigError::Read { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
                let config = Self::default();
                match config.write_to(path) {
                    Ok(()) => info!(path = %path.display(), "created default config"),
                    Err(e) => warn!("{e}; continuing with the default config"),
                }
                config
            }
            Err(e) => {
                error!(path = %path.display(), "{e}; using the default config");
                Self::default()
            }
        }
    }

    fn read(path: &Path) -> Result<Self, ConfigError> {
        let text = fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.to_owned(),
            source,
        })?;
        Self::parse(&text)
    }

    /// Parses config text. Unknown keys are logged as warnings instead of rejected, so a
    /// config file keeps working across versions that add or remove keys.
    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        let (config, unknown) = Self::parse_reporting_unknown(text)?;
        for key in unknown {
            warn!(key, "ignoring unknown config key");
        }
        Ok(config)
    }

    fn parse_reporting_unknown(text: &str) -> Result<(Self, Vec<String>), ConfigError> {
        let mut unknown = Vec::new();
        let deserializer = toml::Deserializer::parse(text)?;
        let config: Self =
            serde_ignored::deserialize(deserializer, |key| unknown.push(key.to_string()))?;
        config.validate()?;
        Ok((config, unknown))
    }

    /// Rejects values that parse but would break the terminal, e.g. a NaN font size
    /// (TOML allows `nan`) or a negative padding that makes the grid wider than the window.
    fn validate(&self) -> Result<(), ConfigError> {
        let size = self.font.size;
        if !FONT_SIZE_RANGE.contains(&size) {
            return Err(ConfigError::Invalid(format!(
                "font.size = {size} must be between {} and {}",
                FONT_SIZE_RANGE.start(),
                FONT_SIZE_RANGE.end()
            )));
        }
        let padding = self.window.padding;
        if !(padding.is_finite() && padding >= 0.0) {
            return Err(ConfigError::Invalid(format!(
                "window.padding = {padding} must be zero or more"
            )));
        }
        Ok(())
    }

    /// Writes this config as a commented TOML file, creating parent directories as needed.
    pub fn write_to(&self, path: &Path) -> Result<(), ConfigError> {
        let write_err = |source| ConfigError::Write {
            path: path.to_owned(),
            source,
        };
        // Serializing plain structs of strings, numbers and tables can't fail.
        let body = toml::to_string_pretty(self).expect("config always serializes to TOML");
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(write_err)?;
        }
        fs::write(path, format!("{CONFIG_HEADER}{body}")).map_err(write_err)
    }
}

#[cfg(test)]
mod tests {
    use super::theme::Rgb;
    use super::*;

    #[test]
    fn defaults_match_the_spec() {
        let c = Config::default();
        assert_eq!(c.font.family, BUNDLED_FONT_FAMILY);
        assert_eq!(c.font.path, None);
        assert_eq!(c.font.size, 14.0);
        assert_eq!(c.shell.program, None);
        assert!(c.shell.args.is_empty());
        assert_eq!(c.scrollback.lines, 10_000);
        assert_eq!(c.cursor.style, CursorStyle::Block);
        assert!(!c.cursor.blink);
        assert_eq!(c.window.padding, 8.0);
        assert!(c.bell.visual);
    }

    #[test]
    fn empty_file_gives_defaults() {
        assert_eq!(Config::parse("").unwrap(), Config::default());
    }

    #[test]
    fn written_defaults_parse_back_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        Config::default().write_to(&path).unwrap();
        let (parsed, unknown) =
            Config::parse_reporting_unknown(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(parsed, Config::default());
        assert!(
            unknown.is_empty(),
            "defaults contain unknown keys: {unknown:?}"
        );
    }

    #[test]
    fn parses_a_sample_config() {
        let text = r##"
            [font]
            path = "/usr/share/fonts/Iosevka.ttf"
            size = 16.5

            [shell]
            program = "/bin/zsh"
            args = ["-l"]

            [cursor]
            style = "beam"
            blink = true

            [theme]
            background = "#000000"

            [theme.normal]
            red = "#ff0000"
        "##;
        let c = Config::parse(text).unwrap();

        assert_eq!(
            c.font.path,
            Some(PathBuf::from("/usr/share/fonts/Iosevka.ttf"))
        );
        assert_eq!(c.font.size, 16.5);
        assert_eq!(
            c.font.family, BUNDLED_FONT_FAMILY,
            "unset keys keep defaults"
        );
        assert_eq!(c.shell.program.as_deref(), Some("/bin/zsh"));
        assert_eq!(c.shell.args, ["-l"]);
        assert_eq!(c.cursor.style, CursorStyle::Beam);
        assert!(c.cursor.blink);
        assert_eq!(c.theme.background, Rgb::new(0, 0, 0));
        assert_eq!(c.theme.normal.red, Rgb::new(0xff, 0, 0));
        assert_eq!(c.theme.normal.green, Theme::default().normal.green);
        assert_eq!(c.theme.foreground, Theme::default().foreground);
        assert_eq!(
            c.scrollback,
            ScrollbackConfig::default(),
            "unset sections keep defaults"
        );
    }

    #[test]
    fn unknown_keys_are_reported_not_rejected() {
        let text = "
            colour = 1
            [font]
            size = 12.0
            wieght = \"bold\"
        ";
        let (c, unknown) = Config::parse_reporting_unknown(text).unwrap();
        assert_eq!(c.font.size, 12.0);
        assert_eq!(unknown, ["colour", "font.wieght"]);
    }

    #[test]
    fn invalid_values_are_errors() {
        for text in [
            "[font]\nsize = \"big\"",
            "[cursor]\nstyle = \"triangle\"",
            "[theme]\nforeground = \"red\"",
            "[font]\nsize = 0.0",
            "[font]\nsize = -12.0",
            "[font]\nsize = nan",
            "[font]\nsize = inf",
            "[font]\nsize = 500.0",
            "[window]\npadding = -20.0",
            "[window]\npadding = nan",
        ] {
            assert!(
                Config::parse(text).is_err(),
                "{text:?} should fail to parse"
            );
        }
    }

    #[test]
    fn malformed_file_falls_back_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "[font\nsize = = 3").unwrap();
        assert_eq!(Config::load_from(&path), Config::default());
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[font\nsize = = 3",
            "a malformed file must not be overwritten"
        );
    }

    #[test]
    fn missing_file_is_created_with_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("config.toml");
        assert_eq!(Config::load_from(&path), Config::default());
        let written = fs::read_to_string(&path).unwrap();
        assert!(written.starts_with("# jterm configuration"));
        assert_eq!(Config::parse(&written).unwrap(), Config::default());
    }
}
