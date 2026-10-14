//! Paints a `RenderSnapshot`. Input, mouse, selection and scroll arrive in Steps 6–9.
//!
//! Text is painted as runs of same-style cells, one galley per run. egui spaces glyphs by
//! the font's real advance, which differs from our pixel-snapped cell width (e.g. 8.4pt vs
//! 8.0pt), so each run gets `extra_letter_spacing = cell width - advance` to keep every
//! glyph on its cell. Characters whose advance differs from the face's (wide CJK, emoji,
//! fallback-font glyphs) would break that, so they're painted one at a time, centered in
//! their cells.

use std::collections::HashMap;

use alacritty_terminal::vte::ansi::CursorShape;
use egui::emath::GuiRounding;
use egui::text::{LayoutJob, TextFormat};
use egui::{
    Color32, FontFamily, FontId, Painter, Pos2, Rect, Stroke, StrokeKind, Vec2, pos2, vec2,
};

use crate::terminal::snapshot::{RenderCell, RenderSnapshot, Style};
use crate::ui::cell_metrics::CellMetrics;
use crate::ui::fonts;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Face {
    Regular,
    Bold,
    Italic,
    BoldItalic,
}

impl Face {
    const ALL: [Face; 4] = [Face::Regular, Face::Bold, Face::Italic, Face::BoldItalic];

    fn of(style: Style) -> Self {
        match (style.bold, style.italic) {
            (false, false) => Face::Regular,
            (true, false) => Face::Bold,
            (false, true) => Face::Italic,
            (true, true) => Face::BoldItalic,
        }
    }

    fn family(self) -> FontFamily {
        match self {
            Face::Regular => FontFamily::Monospace,
            Face::Bold => FontFamily::Name(fonts::BOLD_FAMILY.into()),
            Face::Italic => FontFamily::Name(fonts::ITALIC_FAMILY.into()),
            Face::BoldItalic => FontFamily::Name(fonts::BOLD_ITALIC_FAMILY.into()),
        }
    }
}

/// Per-face font measurements, cached because they only change with the font size or scale.
struct FaceMetrics {
    key: (f32, f32),
    advance: [f32; 4],
    /// Whether a non-ASCII character has the face's normal advance, so it can stay in a run.
    fits: HashMap<(char, Face), bool>,
}

/// Paints the terminal grid. Holds only caches; all terminal state comes from the snapshot.
#[derive(Default)]
pub struct TerminalView {
    faces: Option<FaceMetrics>,
}

/// Where and how big to paint, in points.
#[derive(Clone, Copy)]
pub struct Layout {
    /// Top-left of cell (0, 0), snapped to a physical pixel.
    pub origin: Pos2,
    pub cell: CellMetrics,
    pub font_size: f32,
    pub focused: bool,
}

impl Layout {
    pub fn new(
        ctx: &egui::Context,
        rect: Rect,
        padding: f32,
        cell: CellMetrics,
        font_size: f32,
        focused: bool,
    ) -> Self {
        Self {
            origin: (rect.min + Vec2::splat(padding)).round_to_pixels(ctx.pixels_per_point()),
            cell,
            font_size,
            focused,
        }
    }

    /// The (column, row) under `pos`, clamped to the grid so positions in the padding map
    /// to the nearest edge cell.
    pub fn cell_at(&self, pos: Pos2, cols: usize, rows: usize) -> (usize, usize) {
        let rel = pos - self.origin;
        let clamp = |v: f32, size: f32, n: usize| {
            ((v / size).floor().max(0.0) as usize).min(n.saturating_sub(1))
        };
        (
            clamp(rel.x, self.cell.width, cols),
            clamp(rel.y, self.cell.height, rows),
        )
    }

    pub fn cell_rect(&self, row: usize, col: usize, cells: usize) -> Rect {
        Rect::from_min_size(
            self.origin + vec2(col as f32 * self.cell.width, row as f32 * self.cell.height),
            vec2(cells as f32 * self.cell.width, self.cell.height),
        )
    }

    fn font(&self, face: Face) -> FontId {
        FontId::new(self.font_size, face.family())
    }
}

/// A run of adjacent cells painted as one galley.
struct Run {
    col: usize,
    text: String,
    fg: Color32,
    style: Style,
    /// Only spaces so far; nothing to draw unless decorated.
    blank: bool,
}

impl TerminalView {
    /// Paints `snap` into `rect`: default background, cell backgrounds, text, then cursor.
    pub fn paint(&mut self, painter: &Painter, rect: Rect, layout: Layout, snap: &RenderSnapshot) {
        let ctx = painter.ctx();
        self.measure_faces(ctx, &layout);

        painter.rect_filled(rect, 0.0, snap.background);
        for row in 0..snap.rows {
            self.paint_backgrounds(painter, &layout, row, snap);
        }
        for row in 0..snap.rows {
            self.paint_text(painter, &layout, row, snap.row(row));
        }
        self.paint_cursor(painter, &layout, snap);
        paint_scroll_badge(painter, rect, &layout, snap);
    }

    fn measure_faces(&mut self, ctx: &egui::Context, layout: &Layout) {
        let key = (ctx.pixels_per_point(), layout.font_size);
        if self.faces.as_ref().is_some_and(|f| f.key == key) {
            return;
        }
        let advance =
            Face::ALL.map(|face| ctx.fonts_mut(|f| f.glyph_width(&layout.font(face), 'M')));
        self.faces = Some(FaceMetrics {
            key,
            advance,
            fits: HashMap::new(),
        });
    }

    fn faces(&mut self) -> &mut FaceMetrics {
        // Invariant: `paint` calls `measure_faces` before anything that reaches here.
        self.faces
            .as_mut()
            .expect("faces measured at the start of paint")
    }

    /// Fills runs of cells whose background differs from the default background.
    fn paint_backgrounds(
        &self,
        painter: &Painter,
        layout: &Layout,
        row: usize,
        snap: &RenderSnapshot,
    ) {
        let cells = snap.row(row);
        let mut col = 0;
        while col < cells.len() {
            let bg = cells[col].bg;
            let start = col;
            while col < cells.len() && cells[col].bg == bg {
                col += 1;
            }
            if bg != snap.background {
                painter.rect_filled(layout.cell_rect(row, start, col - start), 0.0, bg);
            }
        }
    }

    fn paint_text(&mut self, painter: &Painter, layout: &Layout, row: usize, cells: &[RenderCell]) {
        let mut run: Option<Run> = None;
        for (col, cell) in cells.iter().enumerate() {
            if cell.spacer {
                continue;
            }
            let decorated = cell.style.underline || cell.style.strikeout;
            let blank = cell.c == ' ' && !decorated;

            if cell.wide || !self.fits(painter.ctx(), layout, cell.c, Face::of(cell.style)) {
                if let Some(r) = run.take() {
                    self.paint_run(painter, layout, row, r);
                }
                let cells = if cell.wide { 2 } else { 1 };
                self.paint_lone_glyph(painter, layout, row, col, cells, cell);
                continue;
            }

            // Spaces are invisible, so they can join any undecorated run whatever their color.
            let joins = run.as_ref().is_some_and(|r| {
                (r.fg == cell.fg && r.style == cell.style)
                    || (blank && !r.style.underline && !r.style.strikeout)
                    || (r.blank && !decorated)
            });
            match &mut run {
                Some(r) if joins => {
                    if r.blank && !blank {
                        // A run of spaces takes the style of its first visible character.
                        r.fg = cell.fg;
                        r.style = cell.style;
                        r.blank = false;
                    }
                    r.text.push(cell.c);
                }
                _ => {
                    if let Some(r) = run.take() {
                        self.paint_run(painter, layout, row, r);
                    }
                    run = Some(Run {
                        col,
                        text: cell.c.to_string(),
                        fg: cell.fg,
                        style: cell.style,
                        blank,
                    });
                }
            }
        }
        if let Some(r) = run {
            self.paint_run(painter, layout, row, r);
        }
    }

    fn paint_run(&mut self, painter: &Painter, layout: &Layout, row: usize, run: Run) {
        if run.blank {
            return;
        }
        let face = Face::of(run.style);
        let spacing = layout.cell.width - self.faces().advance[face as usize];
        // Every char in a run fits one cell, so the char count is the cell span.
        let span = run.text.chars().count();
        let job = LayoutJob::single_section(run.text, text_format(layout, face, run.fg, spacing));
        let galley = painter.layout_job(job);
        painter.galley(layout.cell_rect(row, run.col, 1).min, galley, run.fg);
        decorate(
            painter,
            layout,
            layout.cell_rect(row, run.col, span),
            run.fg,
            run.style,
        );
    }

    fn paint_lone_glyph(
        &mut self,
        painter: &Painter,
        layout: &Layout,
        row: usize,
        col: usize,
        cells: usize,
        cell: &RenderCell,
    ) {
        let face = Face::of(cell.style);
        let job =
            LayoutJob::single_section(cell.c.to_string(), text_format(layout, face, cell.fg, 0.0));
        let galley = painter.layout_job(job);
        let rect = layout.cell_rect(row, col, cells);
        let x = rect.center().x - galley.size().x / 2.0;
        let pos = pos2(x, rect.min.y).round_to_pixels(painter.ctx().pixels_per_point());
        painter.galley(pos, galley, cell.fg);
        decorate(painter, layout, rect, cell.fg, cell.style);
    }

    /// Whether `c` has the face's normal advance and can be laid out inside a run.
    fn fits(&mut self, ctx: &egui::Context, layout: &Layout, c: char, face: Face) -> bool {
        if c.is_ascii() {
            return true;
        }
        // egui lays these out at half a space whatever the font says, so measuring the
        // glyph would wrongly report a fit.
        if matches!(c, '\u{2009}' | '\u{202F}') {
            return false;
        }
        let faces = self.faces();
        if let Some(&fits) = faces.fits.get(&(c, face)) {
            return fits;
        }
        let advance = faces.advance[face as usize];
        let width = ctx.fonts_mut(|f| f.glyph_width(&layout.font(face), c));
        let fits = (width - advance).abs() < 0.01;
        faces.fits.insert((c, face), fits);
        fits
    }

    fn paint_cursor(&mut self, painter: &Painter, layout: &Layout, snap: &RenderSnapshot) {
        let Some(cursor) = snap.cursor else {
            return;
        };
        let cells = if cursor.wide { 2 } else { 1 };
        let rect = layout.cell_rect(cursor.row, cursor.col, cells);
        let px = 1.0 / painter.ctx().pixels_per_point();
        // Thin cursors are 2 physical pixels, but never thinner than 1 point.
        let thickness = (2.0 * px).max(1.0);

        let shape = if layout.focused {
            cursor.shape
        } else {
            CursorShape::HollowBlock
        };
        match shape {
            CursorShape::Block => {
                painter.rect_filled(rect, 0.0, cursor.color);
                // Redraw the character on top in the cell's background color, exactly where
                // the run put it, so it doesn't shift as the cursor moves over it.
                let cell = snap.row(cursor.row)[cursor.col];
                if cell.c != ' ' && !cell.hidden {
                    let face = Face::of(cell.style);
                    if !cell.wide && self.fits(painter.ctx(), layout, cell.c, face) {
                        let run = Run {
                            col: cursor.col,
                            text: cell.c.to_string(),
                            fg: cell.bg,
                            style: cell.style,
                            blank: false,
                        };
                        self.paint_run(painter, layout, cursor.row, run);
                    } else {
                        let inverted = RenderCell {
                            fg: cell.bg,
                            ..cell
                        };
                        self.paint_lone_glyph(
                            painter, layout, cursor.row, cursor.col, cells, &inverted,
                        );
                    }
                }
            }
            CursorShape::Beam => {
                painter.rect_filled(
                    Rect::from_min_size(rect.min, vec2(thickness, rect.height())),
                    0.0,
                    cursor.color,
                );
            }
            CursorShape::Underline => {
                let top = rect.max.y - thickness;
                painter.rect_filled(
                    Rect::from_min_max(pos2(rect.min.x, top), rect.max),
                    0.0,
                    cursor.color,
                );
            }
            CursorShape::HollowBlock => {
                painter.rect_stroke(
                    rect,
                    0.0,
                    Stroke::new(px.max(1.0), cursor.color),
                    StrokeKind::Inside,
                );
            }
            CursorShape::Hidden => {}
        }
    }
}

/// While viewing history, a small "↑ N" badge in the top-right corner shows how far up the
/// view is, so it's obvious why new output isn't appearing. Typing returns to the bottom.
fn paint_scroll_badge(painter: &Painter, rect: Rect, layout: &Layout, snap: &RenderSnapshot) {
    if snap.display_offset == 0 {
        return;
    }
    let font = FontId::monospace(layout.font_size * 0.85);
    // Inverted theme colors, so the badge stands out on light and dark themes alike.
    let (fg, fill) = (snap.background, snap.foreground);
    let galley = painter.layout_no_wrap(format!("↑ {}", snap.display_offset), font, fg);
    let margin = 6.0;
    let badge = Rect::from_min_size(
        pos2(
            rect.max.x - galley.size().x - 3.0 * margin,
            rect.min.y + margin,
        ),
        galley.size() + vec2(2.0 * margin, margin),
    );
    painter.rect_filled(badge, 4.0, fill.gamma_multiply(0.9));
    painter.galley(badge.min + vec2(margin, margin / 2.0), galley, fg);
}

fn text_format(layout: &Layout, face: Face, fg: Color32, spacing: f32) -> TextFormat {
    TextFormat {
        font_id: layout.font(face),
        extra_letter_spacing: spacing,
        color: fg,
        ..TextFormat::default()
    }
}

/// Underline and strikethrough over whole cells. egui's own text decorations span the
/// glyphs, which stop short of the last cell and leave gaps between adjacent runs.
fn decorate(painter: &Painter, layout: &Layout, rect: Rect, fg: Color32, style: Style) {
    if !style.underline && !style.strikeout {
        return;
    }
    let ppp = painter.ctx().pixels_per_point();
    let stroke = Stroke::new((1.0 / ppp).max(1.0), fg);
    // Below the baseline and through the x-height, as fractions of the cell height.
    let line_at = |fraction: f32| {
        let y = rect.min.y + layout.cell.height * fraction;
        painter.hline(rect.x_range(), painter.round_to_pixel_center(y), stroke);
    };
    if style.underline {
        line_at(0.86);
    }
    if style.strikeout {
        line_at(0.55);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs one egui frame with the app's fonts installed.
    fn with_fonts<R>(f: impl FnOnce(&egui::Context) -> R) -> R {
        with_fonts_at(1.0, f)
    }

    fn with_fonts_at<R>(ppp: f32, f: impl FnOnce(&egui::Context) -> R) -> R {
        let ctx = egui::Context::default();
        ctx.set_pixels_per_point(ppp);
        fonts::install(&ctx, &crate::config::FontConfig::default());
        let mut result = None;
        let mut f = Some(f);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            if let Some(f) = f.take() {
                result = Some(f(ui.ctx()));
            }
        });
        output.textures_delta.clear();
        result.expect("run_ui calls the closure once")
    }

    #[test]
    fn runs_stay_on_the_cell_grid() {
        // The Step 3 review finding: without letter spacing a 120-column run drifts ~48pt.
        // Every glyph must start within a pixel of its cell, at 100% and 150% scaling.
        for ppp in [1.0, 1.5] {
            let (xs, cell_width) = with_fonts_at(ppp, |ctx| {
                let cell = CellMetrics::measure(ctx, &FontId::monospace(14.0));
                let layout = Layout::new(ctx, Rect::ZERO, 0.0, cell, 14.0, true);
                let spacing = cell.width - cell.advance;
                let text = "Mi.-W_x0".repeat(15);
                let job = LayoutJob::single_section(
                    text,
                    text_format(&layout, Face::Regular, Color32::WHITE, spacing),
                );
                let galley = ctx.fonts_mut(|f| f.layout_job(job));
                let xs: Vec<f32> = galley.rows[0].row.glyphs.iter().map(|g| g.pos.x).collect();
                (xs, cell.width)
            });
            assert_eq!(xs.len(), 120);
            for (col, x) in xs.iter().enumerate() {
                let expected = col as f32 * cell_width;
                assert!(
                    (x - expected).abs() <= 1.0 / ppp + 1e-3,
                    "ppp {ppp}: glyph {col} at {x}, expected {expected}"
                );
            }
        }
    }

    #[test]
    fn bundled_faces_share_one_advance() {
        // Bold and italic runs reuse the cell grid, so their advance must match regular.
        let advances = with_fonts(|ctx| {
            Face::ALL.map(|face| {
                ctx.fonts_mut(|f| f.glyph_width(&FontId::new(14.0, face.family()), 'M'))
            })
        });
        for a in advances {
            assert!((a - advances[0]).abs() < 0.01, "advances {advances:?}");
        }
    }

    #[test]
    fn wide_and_fallback_chars_dont_fit_runs() {
        let mut view = TerminalView::default();
        let fits = with_fonts(|ctx| {
            let cell = CellMetrics::measure(ctx, &FontId::monospace(14.0));
            let layout = Layout::new(ctx, Rect::ZERO, 0.0, cell, 14.0, true);
            view.measure_faces(ctx, &layout);
            ['a', '─', '日', '😀'].map(|c| view.fits(ctx, &layout, c, Face::Regular))
        });
        // ASCII and box drawing (in JetBrains Mono) fit; CJK and emoji come from fallbacks.
        assert_eq!(fits, [true, true, false, false]);
    }
}
