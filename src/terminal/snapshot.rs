//! Copy of the visible cells and cursor, taken under a short `Term` lock so that rendering
//! never holds the lock. Colors are resolved to final RGB here, against the theme plus any
//! colors the running program changed (OSC 4/10/11), so the renderer needs no terminal state.

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::Term;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::color::{COUNT, Colors};
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor};
use egui::Color32;

use crate::config::theme::{Rgb, Theme};

/// Text attributes the renderer needs. Color attributes (inverse, dim, hidden) are already
/// applied to `fg`/`bg` by the time a cell is in the snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikeout: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderCell {
    pub c: char,
    pub fg: Color32,
    pub bg: Color32,
    pub style: Style,
    /// Occupies this cell and the next one.
    pub wide: bool,
    /// Second half of a wide character, or the padding cell before a wide character that
    /// didn't fit at the end of a line. Never painted as text.
    pub spacer: bool,
    /// SGR 8. `fg == bg` already hides it in runs; the block cursor must not reveal it.
    pub hidden: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderCursor {
    /// Visible row and column.
    pub row: usize,
    pub col: usize,
    pub shape: CursorShape,
    pub color: Color32,
    /// The cursor sits on a wide character and covers two cells.
    pub wide: bool,
}

/// Everything needed to paint one frame of the terminal.
#[derive(Debug, Clone)]
pub struct RenderSnapshot {
    pub cols: usize,
    pub rows: usize,
    /// `rows * cols` cells, row-major, top visible row first.
    pub cells: Vec<RenderCell>,
    /// `None` when hidden or scrolled out of view.
    pub cursor: Option<RenderCursor>,
    /// Lines scrolled back into history. Visible row `r` is grid line `r - display_offset`;
    /// selection and mouse handling (Steps 8–9) need this mapping.
    #[expect(dead_code, reason = "used by scrollback and selection from Step 8")]
    pub display_offset: usize,
    /// Default background, painted behind the whole grid and padding.
    pub background: Color32,
}

impl RenderSnapshot {
    /// Copies the visible screen. Called with the `Term` lock held, so it only copies and
    /// looks up colors; no allocation beyond the cell vector and no egui calls.
    pub fn capture<T: EventListener>(term: &Term<T>, theme: &Theme) -> Self {
        let content = term.renderable_content();
        let palette = Palette::new(theme, content.colors);
        let cols = term.columns();
        let rows = term.screen_lines();
        let offset = content.display_offset as i32;

        let mut cells = vec![RenderCell::blank(palette.background()); rows * cols];
        for indexed in content.display_iter {
            let row = indexed.point.line.0 + offset;
            let col = indexed.point.column.0;
            if row < 0 || row as usize >= rows || col >= cols {
                continue;
            }
            cells[row as usize * cols + col] = palette.cell(indexed.cell);
        }

        let cursor_row = content.cursor.point.line.0 + offset;
        let cursor = (content.cursor.shape != CursorShape::Hidden
            && (0..rows as i32).contains(&cursor_row))
        .then(|| {
            let (row, col) = (cursor_row as usize, content.cursor.point.column.0);
            RenderCursor {
                row,
                col,
                shape: content.cursor.shape,
                color: palette.get(NamedColor::Cursor as usize),
                wide: cells.get(row * cols + col).is_some_and(|c| c.wide),
            }
        });

        Self {
            cols,
            rows,
            cells,
            cursor,

            display_offset: content.display_offset,
            background: palette.background(),
        }
    }

    pub fn row(&self, row: usize) -> &[RenderCell] {
        &self.cells[row * self.cols..(row + 1) * self.cols]
    }
}

impl RenderCell {
    fn blank(bg: Color32) -> Self {
        Self {
            c: ' ',
            fg: bg,
            bg,
            style: Style::default(),
            wide: false,
            spacer: false,
            hidden: false,
        }
    }
}

/// All 269 terminal colors (see `alacritty_terminal::term::color`) resolved to RGB:
/// a color the program set wins, then the theme, then the standard xterm 256-color values.
pub struct Palette([Color32; COUNT]);

impl Palette {
    pub fn new(theme: &Theme, overrides: &Colors) -> Self {
        let mut colors = [Color32::BLACK; COUNT];
        for (i, slot) in colors.iter_mut().enumerate() {
            *slot = match overrides[i] {
                Some(c) => Color32::from_rgb(c.r, c.g, c.b),
                None => default_color(theme, i),
            };
        }
        Self(colors)
    }

    pub fn get(&self, index: usize) -> Color32 {
        self.0[index]
    }

    fn background(&self) -> Color32 {
        self.get(NamedColor::Background as usize)
    }

    fn resolve(&self, color: Color) -> Color32 {
        match color {
            Color::Named(named) => self.get(named as usize),
            Color::Indexed(i) => self.get(usize::from(i)),
            Color::Spec(c) => Color32::from_rgb(c.r, c.g, c.b),
        }
    }

    fn cell(&self, cell: &alacritty_terminal::term::cell::Cell) -> RenderCell {
        let flags = cell.flags;
        let mut fg = self.resolve(cell.fg);
        let mut bg = self.resolve(cell.bg);
        if flags.contains(Flags::DIM) {
            fg = dim(fg);
        }
        if flags.contains(Flags::INVERSE) {
            std::mem::swap(&mut fg, &mut bg);
        }
        if flags.contains(Flags::HIDDEN) {
            fg = bg;
        }
        RenderCell {
            // alacritty stores a literal '\t' in the first cell of a tab stop. egui would lay
            // it out 4 cells wide and shift the rest of the run, so draw control chars as spaces.
            c: if cell.c < ' ' { ' ' } else { cell.c },
            fg,
            bg,
            style: Style {
                bold: flags.contains(Flags::BOLD),
                italic: flags.contains(Flags::ITALIC),
                underline: flags.intersects(Flags::ALL_UNDERLINES),
                strikeout: flags.contains(Flags::STRIKEOUT),
            },
            wide: flags.contains(Flags::WIDE_CHAR),
            spacer: flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER),
            hidden: flags.contains(Flags::HIDDEN),
        }
    }
}

/// Dim text is drawn at two thirds brightness, like alacritty.
fn dim(c: Color32) -> Color32 {
    let f = |v: u8| (u16::from(v) * 2 / 3) as u8;
    Color32::from_rgb(f(c.r()), f(c.g()), f(c.b()))
}

fn rgb(c: Rgb) -> Color32 {
    c.into()
}

/// The color at `index` when the program hasn't overridden it.
fn default_color(theme: &Theme, index: usize) -> Color32 {
    let n = &theme.normal;
    let b = &theme.bright;
    let ansi = [
        n.black, n.red, n.green, n.yellow, n.blue, n.magenta, n.cyan, n.white, b.black, b.red,
        b.green, b.yellow, b.blue, b.magenta, b.cyan, b.white,
    ];
    match index {
        0..16 => rgb(ansi[index]),
        // 6x6x6 color cube, with xterm's channel levels.
        16..232 => {
            const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
            let i = index - 16;
            Color32::from_rgb(LEVELS[i / 36], LEVELS[(i / 6) % 6], LEVELS[i % 6])
        }
        // 24-step grayscale ramp from 8 to 238.
        232..256 => {
            let v = 8 + 10 * (index - 232) as u8;
            Color32::from_rgb(v, v, v)
        }
        _ => match index {
            i if i == NamedColor::Foreground as usize => rgb(theme.foreground),
            i if i == NamedColor::Background as usize => rgb(theme.background),
            i if i == NamedColor::Cursor as usize => rgb(theme.cursor),
            i if i == NamedColor::BrightForeground as usize => rgb(theme.foreground),
            i if i == NamedColor::DimForeground as usize => dim(rgb(theme.foreground)),
            // DimBlack..DimWhite follow the normal colors.
            i => dim(rgb(ansi[i - NamedColor::DimBlack as usize])),
        },
    }
}

#[cfg(test)]
mod tests {
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::term::Config;
    use alacritty_terminal::term::test::TermSize;
    use alacritty_terminal::vte::ansi::Processor;

    use super::*;

    fn term_with(bytes: &[u8]) -> Term<VoidListener> {
        let mut term = Term::new(Config::default(), &TermSize::new(20, 4), VoidListener);
        let mut parser: Processor = Processor::new();
        parser.advance(&mut term, bytes);
        term
    }

    fn snap(bytes: &[u8]) -> RenderSnapshot {
        RenderSnapshot::capture(&term_with(bytes), &Theme::default())
    }

    #[test]
    fn xterm_256_palette() {
        let p = Palette::new(&Theme::default(), &Colors::default());
        let cases = [
            (16, (0, 0, 0)),
            (21, (0, 0, 255)),
            (196, (255, 0, 0)),
            (46, (0, 255, 0)),
            (231, (255, 255, 255)),
            (232, (8, 8, 8)),
            (255, (238, 238, 238)),
        ];
        for (index, (r, g, b)) in cases {
            assert_eq!(p.get(index), Color32::from_rgb(r, g, b), "index {index}");
        }
    }

    #[test]
    fn named_colors_come_from_the_theme() {
        let theme = Theme::default();
        let p = Palette::new(&theme, &Colors::default());
        assert_eq!(p.get(1), rgb(theme.normal.red));
        assert_eq!(p.get(9), rgb(theme.bright.red));
        assert_eq!(
            p.get(NamedColor::Foreground as usize),
            rgb(theme.foreground)
        );
        assert_eq!(
            p.get(NamedColor::Background as usize),
            rgb(theme.background)
        );
        assert_eq!(
            p.get(NamedColor::DimRed as usize),
            dim(rgb(theme.normal.red))
        );
    }

    #[test]
    fn program_set_colors_override_the_theme() {
        // OSC 4 sets palette entry 1; OSC 11 sets the background.
        let s = snap(b"\x1b]4;1;rgb:12/34/56\x07\x1b]11;rgb:01/02/03\x07\x1b[31mX");
        assert_eq!(s.cells[0].fg, Color32::from_rgb(0x12, 0x34, 0x56));
        assert_eq!(s.background, Color32::from_rgb(1, 2, 3));
    }

    #[test]
    fn sgr_colors_resolve() {
        let theme = Theme::default();
        // (escape sequence before 'X', expected fg)
        let cases: [(&[u8], Color32); 5] = [
            (b"", rgb(theme.foreground)),
            (b"\x1b[31m", rgb(theme.normal.red)),
            (b"\x1b[91m", rgb(theme.bright.red)),
            (b"\x1b[38;5;196m", Color32::from_rgb(255, 0, 0)),
            (b"\x1b[38;2;10;20;30m", Color32::from_rgb(10, 20, 30)),
        ];
        for (seq, fg) in cases {
            let s = snap(&[seq, b"X"].concat());
            assert_eq!(s.cells[0].c, 'X');
            assert_eq!(s.cells[0].fg, fg, "sequence {seq:?}");
        }
    }

    #[test]
    fn attributes() {
        let theme = Theme::default();
        let (fg, bg) = (rgb(theme.foreground), rgb(theme.background));

        let s = snap(b"\x1b[7mI\x1b[0m\x1b[8mH\x1b[0m\x1b[2mD\x1b[0m\x1b[1;3;4;9mS");
        let [inverse, hidden, dimmed, styled] = [0, 1, 2, 3].map(|i| s.cells[i]);
        assert_eq!((inverse.fg, inverse.bg), (bg, fg));
        assert_eq!(hidden.fg, hidden.bg);
        assert_eq!(dimmed.fg, dim(fg));
        assert_eq!(
            styled.style,
            Style {
                bold: true,
                italic: true,
                underline: true,
                strikeout: true
            }
        );
    }

    #[test]
    fn tabs_and_control_chars_become_spaces() {
        let s = snap(b"a\tb");
        let row = s.row(0);
        assert_eq!(row[0].c, 'a');
        assert!(
            row[1..8].iter().all(|c| c.c == ' '),
            "tab cells must be spaces"
        );
        assert_eq!(row[8].c, 'b', "b sits at the next tab stop");
    }

    #[test]
    fn hidden_cells_are_flagged() {
        let s = snap(b"\x1b[8mH\x1b[0mV");
        assert!(s.cells[0].hidden);
        assert!(!s.cells[1].hidden);
    }

    #[test]
    fn wide_chars_mark_their_spacer() {
        let s = snap("a日b".as_bytes());
        let row = s.row(0);
        assert_eq!((row[0].c, row[0].wide), ('a', false));
        assert_eq!((row[1].c, row[1].wide), ('日', true));
        assert!(row[2].spacer);
        assert_eq!(row[3].c, 'b');
    }

    #[test]
    fn cursor_position_and_visibility() {
        let s = snap(b"ab\r\ncd");
        let cursor = s.cursor.expect("cursor visible");
        assert_eq!((cursor.row, cursor.col), (1, 2));
        assert_eq!(cursor.shape, CursorShape::Block);

        assert!(
            snap(b"\x1b[?25l").cursor.is_none(),
            "DECTCEM hides the cursor"
        );
        let beam = snap(b"\x1b[6 q").cursor.expect("cursor visible");
        assert_eq!(beam.shape, CursorShape::Beam);
    }
}
