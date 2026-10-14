//! Color palette (16 ANSI colors plus fg/bg/cursor/selection), resolved at snapshot time.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// An sRGB color written as `"#rrggbb"` in the config file, so users can paste hex
/// values straight from any color picker or theme site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Hex constructor for readable palette literals.
    const fn hex(v: u32) -> Self {
        Self::new((v >> 16) as u8, (v >> 8) as u8, v as u8)
    }
}

impl From<Rgb> for egui::Color32 {
    fn from(c: Rgb) -> Self {
        Self::from_rgb(c.r, c.g, c.b)
    }
}

impl fmt::Display for Rgb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

impl FromStr for Rgb {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let invalid = || format!("invalid color {s:?}, expected \"#rrggbb\"");
        let hex = s.strip_prefix('#').ok_or_else(invalid)?;
        if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(invalid());
        }
        let v = u32::from_str_radix(hex, 16).map_err(|_| invalid())?;
        Ok(Self::hex(v))
    }
}

impl Serialize for Rgb {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Rgb {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// Declares one set of the eight base ANSI colors with its own defaults. The normal
/// (0–7) and bright (8–15) sets are separate types so a partial table in the config,
/// such as `[theme.bright]` with only `red`, fills the remaining keys from the right set.
macro_rules! ansi_colors {
    ($(#[$meta:meta])* $name:ident {
        black: $black:literal, red: $red:literal, green: $green:literal, yellow: $yellow:literal,
        blue: $blue:literal, magenta: $magenta:literal, cyan: $cyan:literal, white: $white:literal $(,)?
    }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(default)]
        pub struct $name {
            pub black: Rgb,
            pub red: Rgb,
            pub green: Rgb,
            pub yellow: Rgb,
            pub blue: Rgb,
            pub magenta: Rgb,
            pub cyan: Rgb,
            pub white: Rgb,
        }

        impl Default for $name {
            fn default() -> Self {
                Self {
                    black: Rgb::hex($black),
                    red: Rgb::hex($red),
                    green: Rgb::hex($green),
                    yellow: Rgb::hex($yellow),
                    blue: Rgb::hex($blue),
                    magenta: Rgb::hex($magenta),
                    cyan: Rgb::hex($cyan),
                    white: Rgb::hex($white),
                }
            }
        }
    };
}

ansi_colors!(
    /// ANSI colors 0–7.
    NormalColors {
        black: 0x22252b, red: 0xe06c75, green: 0x98c379, yellow: 0xe5c07b,
        blue: 0x61afef, magenta: 0xc678dd, cyan: 0x56b6c2, white: 0xc8ccd4,
    }
);

ansi_colors!(
    /// ANSI colors 8–15.
    BrightColors {
        black: 0x5c6370, red: 0xef8891, green: 0xb1dd8b, yellow: 0xf0d197,
        blue: 0x8cc8ff, magenta: 0xdda0ef, cyan: 0x7fd3dd, white: 0xf0f2f5,
    }
);

/// The full palette. Missing keys fall back to the default dark theme one by one,
/// so a user can override a single color without restating the rest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Theme {
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor: Rgb,
    pub selection: Rgb,
    pub normal: NormalColors,
    pub bright: BrightColors,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            foreground: Rgb::hex(0xd4d7e0),
            background: Rgb::hex(0x16181d),
            cursor: Rgb::hex(0xe6e8ef),
            selection: Rgb::hex(0x2f3542),
            normal: NormalColors::default(),
            bright: BrightColors::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb_round_trips_through_hex() {
        let c: Rgb = "#1a2B3c".parse().unwrap();
        assert_eq!(c, Rgb::new(0x1a, 0x2b, 0x3c));
        assert_eq!(c.to_string(), "#1a2b3c");
    }

    #[test]
    fn rgb_rejects_bad_input() {
        for bad in ["1a2b3c", "#1a2b3", "#1a2b3c4", "#gggggg", "#+12345", ""] {
            assert!(bad.parse::<Rgb>().is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn partial_bright_table_keeps_bright_defaults() {
        let bright: BrightColors = toml::from_str(r##"red = "#ff0000""##).unwrap();
        assert_eq!(bright.red, Rgb::new(0xff, 0, 0));
        assert_eq!(bright.green, BrightColors::default().green);
    }
}
