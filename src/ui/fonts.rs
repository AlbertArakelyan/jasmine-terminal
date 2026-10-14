//! Registers the terminal font with egui. JetBrains Mono is bundled so the terminal
//! looks right with no setup; `font.path` swaps in a user font for the regular face.

use std::fs;
use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily};
use tracing::{info, warn};

use crate::config::{BUNDLED_FONT_FAMILY, FontConfig};

const REGULAR: &[u8] = include_bytes!("../../assets/fonts/JetBrainsMono-Regular.ttf");
const BOLD: &[u8] = include_bytes!("../../assets/fonts/JetBrainsMono-Bold.ttf");
const ITALIC: &[u8] = include_bytes!("../../assets/fonts/JetBrainsMono-Italic.ttf");
const BOLD_ITALIC: &[u8] = include_bytes!("../../assets/fonts/JetBrainsMono-BoldItalic.ttf");

const REGULAR_NAME: &str = "jterm-regular";

/// egui family names for the styled faces. egui has no notion of bold or italic, so
/// styled cells pick one of these families explicitly.
pub const BOLD_FAMILY: &str = "jterm-bold";
pub const ITALIC_FAMILY: &str = "jterm-italic";
pub const BOLD_ITALIC_FAMILY: &str = "jterm-bold-italic";

/// Installs the configured font as egui's monospace family, plus named families for
/// the bold, italic and bold-italic faces. Each keeps egui's default monospace fonts
/// behind it as fallbacks for glyphs the font lacks, such as emoji.
pub fn install(ctx: &egui::Context, config: &FontConfig) {
    let mut fonts = FontDefinitions::default();
    let fallbacks = fonts
        .families
        .get(&FontFamily::Monospace)
        .cloned()
        .unwrap_or_default();

    fonts
        .font_data
        .insert(REGULAR_NAME.to_owned(), Arc::new(regular_font(config)));
    fonts.families.insert(
        FontFamily::Monospace,
        with_fallbacks(REGULAR_NAME, &fallbacks),
    );

    // Styled faces always come from the bundled font for now, even with a custom `font.path`.
    for (family, bytes) in [
        (BOLD_FAMILY, BOLD),
        (ITALIC_FAMILY, ITALIC),
        (BOLD_ITALIC_FAMILY, BOLD_ITALIC),
    ] {
        fonts
            .font_data
            .insert(family.to_owned(), Arc::new(FontData::from_static(bytes)));
        fonts.families.insert(
            FontFamily::Name(family.into()),
            with_fallbacks(family, &fallbacks),
        );
    }

    ctx.set_fonts(fonts);
}

/// Picks the regular face: the file at `font.path` if it loads, otherwise the bundled font.
fn regular_font(config: &FontConfig) -> FontData {
    if let Some(path) = &config.path {
        match fs::read(path) {
            Ok(bytes) => {
                info!(path = %path.display(), "using font file");
                return FontData::from_owned(bytes);
            }
            Err(e) => {
                warn!(path = %path.display(), "can't read font file ({e}); using {BUNDLED_FONT_FAMILY}")
            }
        }
    } else if config.family != BUNDLED_FONT_FAMILY {
        warn!(
            family = config.family,
            "fonts can't be found by family name yet; set font.path to a font file. Using {BUNDLED_FONT_FAMILY}"
        );
    }
    FontData::from_static(REGULAR)
}

fn with_fallbacks(primary: &str, fallbacks: &[String]) -> Vec<String> {
    std::iter::once(primary.to_owned())
        .chain(fallbacks.iter().cloned())
        .collect()
}
