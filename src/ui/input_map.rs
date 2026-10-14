//! Maps egui key and text events to the byte sequences a shell expects, honoring terminal modes.
//!
//! Sequences follow xterm, which is what `TERM=xterm-256color` promises programs.
//!
//! egui-winit turns Ctrl+C / Ctrl+X / Ctrl+V (Cmd on macOS) into `Event::Copy` / `Cut` /
//! `Paste` and drops the key press, and it drops text typed with Ctrl held. So the control
//! bytes for those three come from the Copy/Cut/Paste events, the other Ctrl combinations
//! from `Event::Key`, and printable text only from `Event::Text` / IME commits.

use alacritty_terminal::term::TermMode;
use egui::{Event, ImeEvent, Key, Modifiers};

/// Terminal modes that change what keys send, read from `TermMode` each frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InputModes {
    /// DECCKM: arrows, Home and End send `ESC O x` instead of `ESC [ x`.
    pub app_cursor: bool,
    /// Pasted text is wrapped in `ESC [200~` ... `ESC [201~` so shells don't run it.
    pub bracketed_paste: bool,
}

impl From<TermMode> for InputModes {
    fn from(mode: TermMode) -> Self {
        Self {
            app_cursor: mode.contains(TermMode::APP_CURSOR),
            bracketed_paste: mode.contains(TermMode::BRACKETED_PASTE),
        }
    }
}

/// What an input event asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    /// Bytes for the PTY.
    Write(Vec<u8>),
    /// The copy shortcut (Ctrl+Shift+C, or Cmd+C on macOS): copy the selection.
    Copy,
    /// Ctrl+C on Windows: copy when there is a selection, otherwise send ^C. That's what
    /// Windows Terminal does, and what Windows users expect from Ctrl+C. Only the app
    /// knows whether something is selected.
    CopyOrInterrupt,
}

/// Tracks the modifier state through the event stream, across frames.
///
/// egui's Copy, Cut, Paste and Text events carry no modifiers, and `InputState::modifiers`
/// is the state at the end of the frame: a fast Ctrl+C has already released Ctrl by then,
/// and "x" then Ctrl+V in one frame would look like a plain paste. egui-winit emits
/// `ModifiersChanged` in order before the keys it affects, so following it (and the
/// modifiers on key events) gives the state at each event.
#[derive(Debug, Default)]
pub struct KeyboardState {
    mods: Modifiers,
}

impl KeyboardState {
    pub fn feed(&mut self, event: &Event, modes: InputModes) -> Option<Input> {
        match event {
            Event::ModifiersChanged(m) | Event::Key { modifiers: m, .. } => self.mods = *m,
            _ => {}
        }
        translate(event, self.mods, modes)
    }
}

/// Translates one egui event, given the modifier state at that event.
fn translate(event: &Event, mods: Modifiers, modes: InputModes) -> Option<Input> {
    let write = |bytes: Vec<u8>| Some(Input::Write(bytes));
    let mac = cfg!(target_os = "macos");
    match event {
        Event::Text(text) => write(with_alt(mods, text.as_bytes())),
        Event::Ime(ImeEvent::Commit(text)) => write(text.clone().into_bytes()),
        Event::Copy if mac || mods.shift => Some(Input::Copy),
        Event::Copy if cfg!(windows) => Some(Input::CopyOrInterrupt),
        Event::Copy => write(vec![0x03]),
        Event::Cut if mac => None,
        // Ctrl+X is ^X; on Windows Shift+Delete also arrives as Cut.
        Event::Cut if mods.ctrl => write(with_alt(mods, &[0x18])),
        Event::Cut => write(b"\x1b[3;2~".to_vec()),
        // Ctrl+V is ^V (vim's visual block; PSReadLine pastes on it itself). Ctrl+Shift+V,
        // Shift+Insert and Cmd+V paste.
        Event::Paste(_) if !mac && mods.ctrl && !mods.shift => write(vec![0x16]),
        Event::Paste(text) => write(paste_bytes(text, modes)),
        Event::Key {
            key,
            pressed: true,
            modifiers,
            ..
        } => key_bytes(*key, *modifiers, modes).map(Input::Write),
        _ => None,
    }
}

/// Prefixes ESC when Alt is held ("meta sends escape"). Not on macOS, where Option types
/// accented characters.
fn with_alt(mods: Modifiers, bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + 1);
    if mods.alt && !cfg!(target_os = "macos") {
        out.push(0x1b);
    }
    out.extend_from_slice(bytes);
    out
}

/// Pasted text for the PTY. With bracketed paste, ESC and ^C are removed so the text
/// can't end the bracket early and run commands (same as alacritty). Without it, newlines
/// become CR, which is what the Enter key sends.
fn paste_bytes(text: &str, modes: InputModes) -> Vec<u8> {
    if modes.bracketed_paste {
        let clean: String = text
            .chars()
            .filter(|&c| c != '\x1b' && c != '\x03')
            .collect();
        [b"\x1b[200~", clean.as_bytes(), b"\x1b[201~"].concat()
    } else {
        text.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
    }
}

/// xterm modifier parameter: 1 + Shift + 2·Alt + 4·Ctrl, or `None` without modifiers.
fn modifier_param(mods: Modifiers) -> Option<u8> {
    let m = u8::from(mods.shift) + 2 * u8::from(mods.alt) + 4 * u8::from(mods.ctrl);
    (m > 0).then_some(m + 1)
}

fn key_bytes(key: Key, mods: Modifiers, modes: InputModes) -> Option<Vec<u8>> {
    // Cmd combinations are app shortcuts on macOS; never leak them as plain keys.
    if mods.mac_cmd {
        return None;
    }
    // On Windows AltGr arrives as Ctrl+Alt, and egui-winit drops the character it types.
    // Turning it into ESC + control code would inject escapes (AltGr+8 = '[' on a German
    // layout would send ESC ESC and leave vim's insert mode), so send nothing instead.
    if cfg!(windows) && mods.ctrl && mods.alt {
        return None;
    }
    // Arrows, Home, End: `ESC [ x` / `ESC O x` (DECCKM), or `ESC [ 1 ; m x` with modifiers.
    let cursor_key = |c: u8| match modifier_param(mods) {
        Some(m) => format!("\x1b[1;{m}{}", c as char).into_bytes(),
        None if modes.app_cursor => vec![0x1b, b'O', c],
        None => vec![0x1b, b'[', c],
    };
    // F1–F4: `ESC O x`, or `ESC [ 1 ; m x` with modifiers.
    let ss3_key = |c: u8| match modifier_param(mods) {
        Some(m) => format!("\x1b[1;{m}{}", c as char).into_bytes(),
        None => vec![0x1b, b'O', c],
    };
    // Editing keys and F5–F12: `ESC [ n ~`, or `ESC [ n ; m ~` with modifiers.
    let tilde_key = |n: u8| match modifier_param(mods) {
        Some(m) => format!("\x1b[{n};{m}~").into_bytes(),
        None => format!("\x1b[{n}~").into_bytes(),
    };

    let bytes = match key {
        Key::ArrowUp => cursor_key(b'A'),
        Key::ArrowDown => cursor_key(b'B'),
        Key::ArrowRight => cursor_key(b'C'),
        Key::ArrowLeft => cursor_key(b'D'),
        Key::Home => cursor_key(b'H'),
        Key::End => cursor_key(b'F'),
        Key::F1 => ss3_key(b'P'),
        Key::F2 => ss3_key(b'Q'),
        Key::F3 => ss3_key(b'R'),
        Key::F4 => ss3_key(b'S'),
        Key::Insert => tilde_key(2),
        Key::Delete => tilde_key(3),
        Key::PageUp => tilde_key(5),
        Key::PageDown => tilde_key(6),
        Key::F5 => tilde_key(15),
        Key::F6 => tilde_key(17),
        Key::F7 => tilde_key(18),
        Key::F8 => tilde_key(19),
        Key::F9 => tilde_key(20),
        Key::F10 => tilde_key(21),
        Key::F11 => tilde_key(23),
        Key::F12 => tilde_key(24),
        Key::Enter => with_alt(mods, b"\r"),
        Key::Escape => with_alt(mods, b"\x1b"),
        Key::Tab if mods.shift => b"\x1b[Z".to_vec(),
        Key::Tab => with_alt(mods, b"\t"),
        // DEL, as most terminals send. Ctrl+Backspace sends BS like xterm and Windows
        // Terminal; PSReadLine deletes a word on it, readline and zsh delete a character.
        Key::Backspace if mods.ctrl => with_alt(mods, &[0x08]),
        Key::Backspace => with_alt(mods, &[0x7f]),
        // Everything else only produces bytes with Ctrl; plain keys arrive as `Event::Text`.
        _ if mods.ctrl => with_alt(mods, &[control_code(key)?]),
        _ => return None,
    };
    Some(bytes)
}

/// C0 control code for Ctrl+key: letters map to 0x01–0x1A, plus the classic punctuation.
fn control_code(key: Key) -> Option<u8> {
    let name = key.name();
    if let [letter @ b'A'..=b'Z'] = name.as_bytes() {
        return Some(letter & 0x1f);
    }
    Some(match key {
        Key::Space | Key::Num2 => 0x00,
        Key::OpenBracket | Key::Num3 => 0x1b,
        Key::Backslash | Key::Num4 => 0x1c,
        Key::CloseBracket | Key::Num5 => 0x1d,
        Key::Num6 => 0x1e,
        Key::Minus | Key::Slash | Key::Num7 => 0x1f,
        Key::Num8 => 0x7f,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Modifiers = Modifiers::NONE;
    const CTRL: Modifiers = Modifiers::CTRL;
    const SHIFT: Modifiers = Modifiers::SHIFT;
    const ALT: Modifiers = Modifiers::ALT;

    fn key(key: Key, modifiers: Modifiers) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    fn bytes(event: &Event, mods: Modifiers, modes: InputModes) -> Option<Vec<u8>> {
        match translate(event, mods, modes)? {
            Input::Write(b) => Some(b),
            Input::Copy | Input::CopyOrInterrupt => None,
        }
    }

    #[test]
    fn special_keys() {
        let normal = InputModes::default();
        let app = InputModes {
            app_cursor: true,
            ..normal
        };
        // (key, modifiers, normal cursor mode, application cursor mode)
        let cases: &[(Key, Modifiers, &[u8], &[u8])] = &[
            (Key::ArrowUp, NONE, b"\x1b[A", b"\x1bOA"),
            (Key::ArrowDown, NONE, b"\x1b[B", b"\x1bOB"),
            (Key::ArrowRight, NONE, b"\x1b[C", b"\x1bOC"),
            (Key::ArrowLeft, NONE, b"\x1b[D", b"\x1bOD"),
            (Key::Home, NONE, b"\x1b[H", b"\x1bOH"),
            (Key::End, NONE, b"\x1b[F", b"\x1bOF"),
            // Modifiers always use the CSI form, even in application mode.
            (Key::ArrowUp, CTRL, b"\x1b[1;5A", b"\x1b[1;5A"),
            (Key::ArrowLeft, SHIFT, b"\x1b[1;2D", b"\x1b[1;2D"),
            (Key::ArrowRight, ALT, b"\x1b[1;3C", b"\x1b[1;3C"),
            (Key::Enter, NONE, b"\r", b"\r"),
            (Key::Enter, ALT, b"\x1b\r", b"\x1b\r"),
            (Key::Backspace, NONE, b"\x7f", b"\x7f"),
            (Key::Backspace, CTRL, b"\x08", b"\x08"),
            (Key::Tab, NONE, b"\t", b"\t"),
            (Key::Tab, SHIFT, b"\x1b[Z", b"\x1b[Z"),
            (Key::Escape, NONE, b"\x1b", b"\x1b"),
            (Key::Insert, NONE, b"\x1b[2~", b"\x1b[2~"),
            (Key::Delete, NONE, b"\x1b[3~", b"\x1b[3~"),
            (Key::Delete, CTRL, b"\x1b[3;5~", b"\x1b[3;5~"),
            (Key::PageUp, NONE, b"\x1b[5~", b"\x1b[5~"),
            (Key::PageDown, NONE, b"\x1b[6~", b"\x1b[6~"),
            (Key::F1, NONE, b"\x1bOP", b"\x1bOP"),
            (Key::F4, NONE, b"\x1bOS", b"\x1bOS"),
            (Key::F1, SHIFT, b"\x1b[1;2P", b"\x1b[1;2P"),
            (Key::F5, NONE, b"\x1b[15~", b"\x1b[15~"),
            (Key::F6, NONE, b"\x1b[17~", b"\x1b[17~"),
            (Key::F10, NONE, b"\x1b[21~", b"\x1b[21~"),
            (Key::F11, NONE, b"\x1b[23~", b"\x1b[23~"),
            (Key::F12, CTRL, b"\x1b[24;5~", b"\x1b[24;5~"),
        ];
        for &(k, m, expect_normal, expect_app) in cases {
            assert_eq!(
                bytes(&key(k, m), m, normal).as_deref(),
                Some(expect_normal),
                "{k:?} {m:?}"
            );
            assert_eq!(
                bytes(&key(k, m), m, app).as_deref(),
                Some(expect_app),
                "{k:?} {m:?} app"
            );
        }
    }

    #[test]
    fn ctrl_combinations() {
        let m = InputModes::default();
        let alt_ctrl = Modifiers {
            alt: true,
            ctrl: true,
            ..NONE
        };
        let cases: &[(Key, Modifiers, &[u8])] = &[
            (Key::A, CTRL, b"\x01"),
            (Key::D, CTRL, b"\x04"),
            (Key::L, CTRL, b"\x0c"),
            (Key::R, CTRL, b"\x12"),
            (Key::Z, CTRL, b"\x1a"),
            (Key::Space, CTRL, b"\x00"),
            (Key::OpenBracket, CTRL, b"\x1b"),
            (Key::Backslash, CTRL, b"\x1c"),
            (Key::CloseBracket, CTRL, b"\x1d"),
            (Key::Num6, CTRL, b"\x1e"),
            (Key::Minus, CTRL, b"\x1f"),
        ];
        for &(k, mods, expect) in cases {
            assert_eq!(
                bytes(&key(k, mods), mods, m).as_deref(),
                Some(expect),
                "{k:?} {mods:?}"
            );
        }
        // Ctrl+Alt is meta + control elsewhere, but AltGr on Windows, where it must send nothing.
        let ctrl_alt_d = bytes(&key(Key::D, alt_ctrl), alt_ctrl, m);
        if cfg!(windows) {
            assert_eq!(ctrl_alt_d, None);
            let alt_gr_8 = bytes(&key(Key::OpenBracket, alt_ctrl), alt_ctrl, m);
            assert_eq!(
                alt_gr_8, None,
                "AltGr+8 ('[' on German layouts) must not send ESC ESC"
            );
        } else {
            assert_eq!(ctrl_alt_d.as_deref(), Some(&b"\x1b\x04"[..]));
        }
        // Cmd combinations (macOS) never leak as plain keys.
        let cmd = Modifiers {
            mac_cmd: true,
            command: true,
            ..NONE
        };
        assert_eq!(bytes(&key(Key::Enter, cmd), cmd, m), None);
        // Plain printable keys produce nothing here: their text arrives as Event::Text.
        assert_eq!(bytes(&key(Key::A, NONE), NONE, m), None);
        assert_eq!(bytes(&key(Key::A, SHIFT), SHIFT, m), None);
        // Releases are ignored.
        let release = Event::Key {
            key: Key::Enter,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: NONE,
        };
        assert_eq!(bytes(&release, NONE, m), None);
    }

    #[test]
    fn text_and_ime() {
        let m = InputModes::default();
        let text = Event::Text("ls -la".to_owned());
        assert_eq!(bytes(&text, NONE, m).as_deref(), Some(&b"ls -la"[..]));
        let unicode = Event::Text("日本".to_owned());
        assert_eq!(bytes(&unicode, NONE, m), Some("日本".as_bytes().to_vec()));
        let ime = Event::Ime(ImeEvent::Commit("한".to_owned()));
        assert_eq!(bytes(&ime, NONE, m), Some("한".as_bytes().to_vec()));
        if !cfg!(target_os = "macos") {
            let alt_b = Event::Text("b".to_owned());
            assert_eq!(bytes(&alt_b, ALT, m).as_deref(), Some(&b"\x1bb"[..]));
        }
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn egui_clipboard_events_become_control_codes() {
        let m = InputModes::default();
        let ctrl_shift = Modifiers {
            ctrl: true,
            shift: true,
            ..NONE
        };
        // Ctrl+C interrupts (on Windows: copies if something is selected); Ctrl+Shift+C copies.
        let ctrl_c = if cfg!(windows) {
            Input::CopyOrInterrupt
        } else {
            Input::Write(vec![0x03])
        };
        assert_eq!(translate(&Event::Copy, CTRL, m), Some(ctrl_c));
        assert_eq!(translate(&Event::Copy, ctrl_shift, m), Some(Input::Copy));
        assert_eq!(bytes(&Event::Cut, CTRL, m), Some(vec![0x18]));
        assert_eq!(
            bytes(&Event::Cut, SHIFT, m).as_deref(),
            Some(&b"\x1b[3;2~"[..])
        );
        let paste = Event::Paste("hi".to_owned());
        assert_eq!(bytes(&paste, CTRL, m), Some(vec![0x16]));
        assert_eq!(bytes(&paste, ctrl_shift, m).as_deref(), Some(&b"hi"[..]));
        assert_eq!(
            bytes(&paste, SHIFT, m).as_deref(),
            Some(&b"hi"[..]),
            "Shift+Insert"
        );
    }

    #[test]
    fn paste_modes() {
        let plain = InputModes::default();
        let bracketed = InputModes {
            bracketed_paste: true,
            ..plain
        };
        assert_eq!(paste_bytes("a\nb\r\nc", plain), b"a\rb\rc");
        assert_eq!(
            paste_bytes("echo hi\n", bracketed),
            b"\x1b[200~echo hi\n\x1b[201~"
        );
        // An embedded end marker can't escape the bracket.
        assert_eq!(
            paste_bytes("x\x1b[201~rm -rf ~\x03", bracketed),
            b"\x1b[200~x[201~rm -rf ~\x1b[201~"
        );
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn modifiers_follow_the_event_stream() {
        let m = InputModes::default();
        let mut kb = KeyboardState::default();
        let mut feed = |e: Event| match kb.feed(&e, m) {
            Some(Input::Write(b)) => Some(b),
            Some(Input::Copy) => Some(b"<copy>".to_vec()),
            // Without a selection this is ^C, which is what the app then sends.
            Some(Input::CopyOrInterrupt) => Some(vec![0x03]),
            None => None,
        };
        // "x" then Ctrl+V in one frame: the paste must see Ctrl, so it's ^V, not a paste.
        assert_eq!(feed(key(Key::X, NONE)), None);
        assert_eq!(feed(Event::ModifiersChanged(CTRL)), None);
        assert_eq!(feed(Event::Paste("clip".into())), Some(vec![0x16]));
        // Ctrl+Shift+C copies even if the frame ends after Shift was released.
        let ctrl_shift = Modifiers {
            ctrl: true,
            shift: true,
            ..NONE
        };
        assert_eq!(feed(Event::ModifiersChanged(ctrl_shift)), None);
        assert_eq!(feed(Event::Copy), Some(b"<copy>".to_vec()));
        assert_eq!(feed(Event::ModifiersChanged(CTRL)), None);
        // The state carries over: a Copy with only Ctrl held is ^C.
        assert_eq!(feed(Event::Copy), Some(vec![0x03]));
        assert_eq!(feed(Event::ModifiersChanged(NONE)), None);
        assert_eq!(feed(Event::Text("a".into())), Some(b"a".to_vec()));
    }
}
