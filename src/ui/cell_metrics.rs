//! Font metrics (cell width/height) and conversion of a pixel rect into a grid size.
//!
//! Cell sizes are snapped to whole physical pixels. Without that, fractional cell widths
//! accumulate across a row and glyphs drift off the pixel grid, which shows up as blurry
//! text and uneven gaps between columns.

use egui::{FontId, Rect};

/// Size of one terminal cell in points, snapped so that it covers a whole number of
/// physical pixels at the current `pixels_per_point`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellMetrics {
    pub width: f32,
    pub height: f32,
}

/// Terminal grid size in cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridSize {
    pub cols: u16,
    pub rows: u16,
}

impl CellMetrics {
    /// Measures the monospace font: the advance width of `M` and egui's row height.
    /// Only valid once egui has run a frame, because fonts don't exist before that.
    pub fn measure(ctx: &egui::Context, font_id: &FontId) -> Self {
        let (advance, row_height) =
            ctx.fonts_mut(|f| (f.glyph_width(font_id, 'M'), f.row_height(font_id)));
        Self::from_font(advance, row_height, ctx.pixels_per_point())
    }

    /// Builds metrics from raw font measurements in points. Each dimension is rounded to
    /// the nearest physical pixel, with a minimum of one pixel.
    pub fn from_font(advance: f32, row_height: f32, pixels_per_point: f32) -> Self {
        let snap = |points: f32| (points * pixels_per_point).round().max(1.0) / pixels_per_point;
        Self {
            width: snap(advance),
            height: snap(row_height),
        }
    }

    /// How many whole cells fit in `rect` after removing `padding` on every side.
    /// Always at least 1×1 so the terminal never gets a zero-sized grid, which
    /// `alacritty_terminal` and the PTY can't represent.
    pub fn grid_size(&self, rect: Rect, padding: f32) -> GridSize {
        let fit = |available: f32, cell: f32| {
            // The epsilon absorbs float error, e.g. 840.0 / 8.4 = 99.99999 must give 100.
            let n = ((available - 2.0 * padding).max(0.0) / cell + 1e-3).floor();
            n.clamp(1.0, f32::from(u16::MAX)) as u16
        };
        GridSize {
            cols: fit(rect.width(), self.width),
            rows: fit(rect.height(), self.height),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{pos2, vec2};

    fn rect(w: f32, h: f32) -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), vec2(w, h))
    }

    /// Measures with a real egui context and the installed fonts, as the app does on its
    /// first frame. Fonts set before the first pass must already be active during it.
    fn measure_headless(config: &crate::config::FontConfig, ppp: f32) -> CellMetrics {
        let ctx = egui::Context::default();
        ctx.set_pixels_per_point(ppp);
        crate::ui::fonts::install(&ctx, config);
        let mut measured = None;
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            measured = Some(CellMetrics::measure(
                ui.ctx(),
                &FontId::monospace(config.size),
            ));
        });
        // There's no renderer to upload the font atlas to; egui asserts that pending
        // texture updates are consumed or cleared explicitly.
        output.textures_delta.clear();
        measured.expect("run_ui calls the closure once")
    }

    #[test]
    fn measures_the_bundled_font() {
        // JetBrains Mono: advance 600/1000 em, line height 1.32 em. At 14pt that's
        // 8.4pt x ~18.5pt; egui's default monospace font would give a shorter row.
        let config = crate::config::FontConfig::default();
        assert_eq!(
            measure_headless(&config, 1.0),
            CellMetrics {
                width: 8.0,
                height: 18.0
            }
        );
        let m = measure_headless(&config, 2.0);
        assert_eq!((m.width * 2.0, m.height * 2.0), (17.0, 37.0));
    }

    #[test]
    fn snaps_to_physical_pixels() {
        // 8.4pt at 1x rounds to 8px; at 1.5x, 12.6px rounds to 13px = 8.666..pt.
        assert_eq!(
            CellMetrics::from_font(8.4, 17.0, 1.0),
            CellMetrics {
                width: 8.0,
                height: 17.0
            }
        );
        let m = CellMetrics::from_font(8.4, 17.0, 1.5);
        assert_eq!(m.width * 1.5, 13.0);
        assert_eq!(m.height * 1.5, 26.0);
    }

    #[test]
    fn never_collapses_to_zero() {
        let m = CellMetrics::from_font(0.1, 0.0, 1.0);
        assert_eq!((m.width, m.height), (1.0, 1.0));
    }

    #[test]
    fn rect_to_grid_size() {
        let m = CellMetrics {
            width: 8.0,
            height: 16.0,
        };
        // (rect w, rect h, padding) -> (cols, rows)
        let cases = [
            ((800.0, 480.0, 0.0), (100, 30)),
            ((816.0, 496.0, 8.0), (100, 30)), // padding on both sides
            ((823.9, 511.9, 8.0), (100, 30)), // partial cells are dropped
            ((824.0, 512.0, 8.0), (101, 31)), // exactly one more cell
            ((1000.0, 650.0, 8.0), (123, 39)), // the default window
            ((10.0, 10.0, 8.0), (1, 1)),      // padding larger than the rect
            ((0.0, 0.0, 0.0), (1, 1)),        // empty rect
        ];
        for ((w, h, padding), (cols, rows)) in cases {
            assert_eq!(
                m.grid_size(rect(w, h), padding),
                GridSize { cols, rows },
                "rect {w}x{h}, padding {padding}"
            );
        }
    }

    #[test]
    fn grid_size_tolerates_float_error() {
        let m = CellMetrics {
            width: 8.4,
            height: 16.8,
        };
        assert_eq!(m.grid_size(rect(840.0, 504.0), 0.0).cols, 100);
        assert_eq!(m.grid_size(rect(840.0, 504.0), 0.0).rows, 30);
    }

    #[test]
    fn huge_rect_is_clamped() {
        let m = CellMetrics {
            width: 1.0,
            height: 1.0,
        };
        let g = m.grid_size(rect(1.0e6, 1.0e6), 0.0);
        assert_eq!((g.cols, g.rows), (u16::MAX, u16::MAX));
    }
}
