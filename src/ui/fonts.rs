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

/// egui's icon font. It puts egui's own UI icons on Private Use Area codepoints, which
/// clash with Nerd Font glyphs: oh-my-posh's Windows logo (U+E62A) rendered as a Chrome
/// logo. In a terminal, PUA glyphs mean whatever the user's icon font says, so a missing
/// one must show as missing rather than as an unrelated icon.
const EGUI_ICON_FONT: &str = "emoji-icon-font";

/// Installs the configured font as egui's monospace family, plus named families for
/// the bold, italic and bold-italic faces. Each keeps egui's default monospace fonts
/// (minus its icon font) behind it as fallbacks for glyphs the font lacks, such as emoji.
pub fn install(ctx: &egui::Context, config: &FontConfig) {
    let mut fonts = FontDefinitions::default();
    let fallbacks = terminal_fallbacks(&fonts);

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

/// egui's default monospace fallback chain, without its icon font. egui's own UI still
/// uses the icon font through the proportional family, which is left untouched.
fn terminal_fallbacks(fonts: &FontDefinitions) -> Vec<String> {
    fonts
        .families
        .get(&FontFamily::Monospace)
        .into_iter()
        .flatten()
        .filter(|name| name.as_str() != EGUI_ICON_FONT)
        .cloned()
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_fallbacks_drop_only_the_icon_font() {
        let fallbacks = terminal_fallbacks(&FontDefinitions::default());
        assert!(!fallbacks.iter().any(|f| f == EGUI_ICON_FONT));
        assert!(
            fallbacks.iter().any(|f| f == "NotoEmoji-Regular"),
            "real emoji stay"
        );
    }

    #[test]
    fn nerd_font_codepoints_are_not_drawn_as_egui_icons() {
        let ctx = egui::Context::default();
        install(&ctx, &FontConfig::default());
        let mut has = None;
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let mono = egui::FontId::monospace(14.0);
            has = Some(ui.ctx().fonts_mut(|f| {
                // U+E62A: Nerd Font's Windows logo, egui's Chrome icon.
                (f.has_glyph(&mono, '\u{E62A}'), f.has_glyph(&mono, '😀'))
            }));
        });
        output.textures_delta.clear();
        assert_eq!(has, Some((false, true)));
    }
}
