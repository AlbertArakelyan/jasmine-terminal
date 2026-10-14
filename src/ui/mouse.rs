//! Mouse wheel handling and xterm mouse reports.
//!
//! The wheel does one of three things, the same as other terminals:
//! 1. When a program enabled mouse reporting (vim `mouse=a`, htop, tmux), it's sent to the
//!    program as wheel button presses. Holding Shift overrides this and scrolls locally.
//! 2. On the alternate screen without mouse reporting (less, man), it sends arrow keys, so
//!    those programs scroll instead of the (empty) scrollback.
//! 3. Otherwise it scrolls the scrollback.

use alacritty_terminal::term::TermMode;
use egui::{Modifiers, MouseWheelUnit};

/// Lines per wheel notch, like xterm and alacritty's default.
const LINES_PER_NOTCH: f32 = 3.0;

/// Turns wheel deltas into whole lines. Trackpads send small pixel deltas, so the
/// remainder is kept between events instead of being rounded away.
#[derive(Debug, Default)]
pub struct WheelAccumulator {
    pending: f32,
}

impl WheelAccumulator {
    /// Lines to scroll: positive is up (towards history), matching egui's sign convention
    /// where positive `delta.y` reveals content above.
    pub fn lines(
        &mut self,
        unit: MouseWheelUnit,
        delta_y: f32,
        cell_height: f32,
        rows: usize,
    ) -> i32 {
        let delta = match unit {
            MouseWheelUnit::Line => delta_y * LINES_PER_NOTCH,
            MouseWheelUnit::Point => delta_y / cell_height.max(1.0),
            MouseWheelUnit::Page => delta_y * rows as f32,
        };
        if !delta.is_finite() {
            return 0;
        }
        // A leftover fraction from the other direction would swallow the start of a
        // reversed scroll, so drop it when the direction changes.
        if delta * self.pending < 0.0 {
            self.pending = 0.0;
        }
        self.pending += delta;
        let whole = self.pending.trunc();
        self.pending -= whole;
        whole as i32
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WheelAction {
    /// Scroll the scrollback by this many lines (positive = up).
    Scroll(i32),
    /// Bytes for the PTY.
    Write(Vec<u8>),
}

/// Decides what `lines` of wheel movement do. `cell` is the (column, row) under the
/// pointer, needed for mouse reports. What's sent to a program is capped at `max_lines`
/// (the page height, like alacritty), so a bogus huge delta can't flood it.
pub fn wheel_action(
    lines: i32,
    mode: TermMode,
    mods: Modifiers,
    cell: Option<(usize, usize)>,
    max_lines: usize,
) -> Option<WheelAction> {
    if lines == 0 {
        return None;
    }
    let up = lines > 0;
    let count = (lines.unsigned_abs() as usize).min(max_lines.max(1));
    if mode.intersects(TermMode::MOUSE_MODE) && !mods.shift {
        // Buttons 64/65 are wheel up/down; one report per line.
        let button = if up { 64 } else { 65 };
        let report = mouse_report(button, cell?, true, mods, mode)?;
        return Some(WheelAction::Write(report.repeat(count)));
    }
    if mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL) && !mods.shift {
        let arrow: &[u8] = match (up, mode.contains(TermMode::APP_CURSOR)) {
            (true, true) => b"\x1bOA",
            (true, false) => b"\x1b[A",
            (false, true) => b"\x1bOB",
            (false, false) => b"\x1b[B",
        };
        return Some(WheelAction::Write(arrow.repeat(count)));
    }
    Some(WheelAction::Scroll(lines))
}

/// An xterm mouse report for `button` at the 0-based `(column, row)`, with Shift (+4),
/// Alt (+8) and Ctrl (+16) added to the button code as xterm does, so vim sees e.g.
/// `<C-ScrollWheelUp>`.
///
/// SGR encoding (`ESC [ < b ; x ; y M/m`) when the program asked for it, which has no
/// coordinate limit. Otherwise the legacy `ESC [ M b x y` form, where each value is offset
/// by 32 and stored in one byte (or 2-byte UTF-8 in UTF-8 mode). Positions those can't
/// represent get no report (`None`), like alacritty, rather than a wrong position.
pub fn mouse_report(
    button: u8,
    (col, row): (usize, usize),
    pressed: bool,
    mods: Modifiers,
    mode: TermMode,
) -> Option<Vec<u8>> {
    let modifier_bits =
        4 * u8::from(mods.shift) + 8 * u8::from(mods.alt) + 16 * u8::from(mods.ctrl);
    if mode.contains(TermMode::SGR_MOUSE) {
        let end = if pressed { 'M' } else { 'm' };
        let code = button + modifier_bits;
        return Some(format!("\x1b[<{code};{};{}{end}", col + 1, row + 1).into_bytes());
    }
    // Legacy reports signal a release as button 3 rather than with a final 'm'.
    let code = if pressed { button } else { 3 } + modifier_bits;
    let limit = if mode.contains(TermMode::UTF8_MOUSE) {
        2015
    } else {
        223
    };
    if col >= limit || row >= limit {
        return None;
    }
    let mut out = b"\x1b[M".to_vec();
    out.push(32 + code);
    for coordinate in [col, row] {
        let value = 32 + 1 + coordinate;
        if mode.contains(TermMode::UTF8_MOUSE) {
            // Within the limit, so this is a valid char of at most 2 UTF-8 bytes.
            let c = char::from_u32(value as u32).unwrap_or(' ');
            let mut buf = [0; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        } else {
            out.push(value as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Modifiers = Modifiers::NONE;

    #[test]
    fn accumulates_fractional_scrolls() {
        let mut acc = WheelAccumulator::default();
        // One wheel notch is three lines.
        assert_eq!(acc.lines(MouseWheelUnit::Line, 1.0, 18.0, 35), 3);
        assert_eq!(acc.lines(MouseWheelUnit::Line, -1.0, 18.0, 35), -3);
        // Trackpad pixels: 10pt + 10pt with 18pt rows is one line, with 2pt carried over.
        assert_eq!(acc.lines(MouseWheelUnit::Point, 10.0, 18.0, 35), 0);
        assert_eq!(acc.lines(MouseWheelUnit::Point, 10.0, 18.0, 35), 1);
        assert_eq!(acc.lines(MouseWheelUnit::Point, 16.0, 18.0, 35), 1);
        assert_eq!(acc.lines(MouseWheelUnit::Page, -1.0, 18.0, 35), -35);
    }

    #[test]
    fn accumulator_resets_on_reversal_and_survives_bad_input() {
        let mut acc = WheelAccumulator::default();
        assert_eq!(acc.lines(MouseWheelUnit::Point, 16.0, 18.0, 35), 0); // 0.89 pending
        // Reversing drops the pending 0.89 instead of swallowing the first line down.
        assert_eq!(acc.lines(MouseWheelUnit::Point, -18.0, 18.0, 35), -1);
        // A zero cell height or a non-finite delta must not poison the accumulator.
        assert_eq!(acc.lines(MouseWheelUnit::Point, 5.0, 0.0, 35), 5);
        assert_eq!(acc.lines(MouseWheelUnit::Line, f32::INFINITY, 18.0, 35), 0);
        assert_eq!(acc.lines(MouseWheelUnit::Line, 1.0, 18.0, 35), 3);
    }

    #[test]
    fn wheel_actions_by_mode() {
        let normal = TermMode::empty();
        let less = TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL;
        let less_app = less | TermMode::APP_CURSOR;
        let vim_mouse = TermMode::ALT_SCREEN | TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
        let cell = Some((4, 2));
        let act = |lines, mode, mods| wheel_action(lines, mode, mods, cell, 35);
        let write = |b: &[u8]| Some(WheelAction::Write(b.to_vec()));

        assert_eq!(act(0, normal, NONE), None);
        assert_eq!(act(3, normal, NONE), Some(WheelAction::Scroll(3)));
        assert_eq!(act(-3, normal, NONE), Some(WheelAction::Scroll(-3)));
        // Alternate screen: arrows, honoring application cursor mode.
        assert_eq!(act(2, less, NONE), write(b"\x1b[A\x1b[A"));
        assert_eq!(act(-1, less_app, NONE), write(b"\x1bOB"));
        // Mouse reporting: wheel buttons at the pointer cell (1-based in the report).
        assert_eq!(act(1, vim_mouse, NONE), write(b"\x1b[<64;5;3M"));
        assert_eq!(
            act(-2, vim_mouse, NONE),
            write(b"\x1b[<65;5;3M\x1b[<65;5;3M")
        );
        // Ctrl adds 16 to the button: vim sees <C-ScrollWheelUp>.
        assert_eq!(act(1, vim_mouse, Modifiers::CTRL), write(b"\x1b[<80;5;3M"));
        // Shift always scrolls the scrollback.
        assert_eq!(
            act(3, vim_mouse, Modifiers::SHIFT),
            Some(WheelAction::Scroll(3))
        );
        assert_eq!(act(3, less, Modifiers::SHIFT), Some(WheelAction::Scroll(3)));
        // Writes to a program are capped at a page; no pointer means no mouse report.
        assert_eq!(
            wheel_action(10_000, less, NONE, cell, 35),
            write(&b"\x1b[A".repeat(35))
        );
        assert_eq!(wheel_action(1, vim_mouse, NONE, None, 35), None);
    }

    #[test]
    fn report_encodings() {
        let sgr = TermMode::SGR_MOUSE;
        assert_eq!(
            mouse_report(0, (0, 0), true, NONE, sgr).unwrap(),
            b"\x1b[<0;1;1M"
        );
        assert_eq!(
            mouse_report(0, (0, 0), false, NONE, sgr).unwrap(),
            b"\x1b[<0;1;1m"
        );
        // SGR has no coordinate limit.
        assert_eq!(
            mouse_report(0, (499, 0), true, NONE, sgr).unwrap(),
            b"\x1b[<0;500;1M"
        );
        // Legacy: 32 + value, release is button 3, modifiers still added.
        let legacy = TermMode::empty();
        assert_eq!(
            mouse_report(0, (4, 2), true, NONE, legacy).unwrap(),
            b"\x1b[M\x20\x25\x23"
        );
        assert_eq!(
            mouse_report(0, (4, 2), false, NONE, legacy).unwrap(),
            b"\x1b[M\x23\x25\x23"
        );
        assert_eq!(
            mouse_report(0, (4, 2), true, Modifiers::ALT, legacy).unwrap()[3],
            32 + 8
        );
        // Positions legacy can't encode get no report instead of a wrong one.
        assert_eq!(
            mouse_report(64, (222, 0), true, NONE, legacy).unwrap()[4],
            255
        );
        assert_eq!(mouse_report(64, (223, 0), true, NONE, legacy), None);
        // UTF-8 mode reaches further, as 2-byte UTF-8.
        let utf8 = mouse_report(64, (300, 0), true, NONE, TermMode::UTF8_MOUSE).unwrap();
        assert_eq!(&utf8[4..6], "ō".as_bytes(), "32 + 1 + 300 = 333 = U+014D");
        assert_eq!(
            mouse_report(64, (2015, 0), true, NONE, TermMode::UTF8_MOUSE),
            None
        );
    }
}
