//! `EventListener` impl that forwards terminal events to the UI thread and requests a repaint,
//! so the UI only redraws when the PTY has produced something.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, OnceLock};

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::event_loop::{EventLoopSender, Msg};

/// Called on the PTY I/O thread, often while that thread holds the `Term` lock, so it
/// must stay cheap and never block.
///
/// Lock order: this takes egui's context lock (`request_repaint`) while the `Term` lock may
/// be held. UI code must therefore never lock `Term` inside an egui context closure
/// (`ctx.input(|i| ..)`, `ui.input(..)`, `ctx.fonts(..)`, `ctx.memory(..)`), or the two
/// threads can deadlock.
#[derive(Clone)]
pub struct EventProxy {
    tx: Sender<Event>,
    ctx: egui::Context,
    /// Set once the event loop exists. Replies are written from here rather than by the
    /// UI, so they don't wait for a frame (a hidden window may get none).
    pty: Arc<OnceLock<EventLoopSender>>,
    /// Coalesces `Wakeup`s: at most one is queued until the UI drains the queue, so a
    /// hidden window running `yes` can't grow the queue without bound.
    wakeup_pending: Arc<AtomicBool>,
}

impl EventProxy {
    pub fn new(tx: Sender<Event>, ctx: egui::Context) -> Self {
        Self {
            tx,
            ctx,
            pty: Arc::default(),
            wakeup_pending: Arc::default(),
        }
    }

    /// Connects the proxy to the event loop so terminal replies can be written back.
    pub fn connect(&self, sender: EventLoopSender) {
        // A second call would mean a second event loop for one terminal; keep the first.
        let _ = self.pty.set(sender);
    }

    /// Called by the UI before draining events, so the next PTY read queues a new `Wakeup`.
    pub fn clear_wakeup(&self) {
        self.wakeup_pending.store(false, Ordering::Release);
    }
}

impl EventListener for EventProxy {
    fn send_event(&self, event: Event) {
        match event {
            // Answers to terminal queries (DA, DSR/cursor position, DECRQM, ...). Programs such
            // as fish, vim and crossterm apps wait for these, so they must reach the PTY.
            Event::PtyWrite(text) => {
                if let Some(pty) = self.pty.get() {
                    let _ = pty.send(Msg::Input(text.into_bytes().into()));
                }
                return;
            }
            Event::Wakeup if self.wakeup_pending.swap(true, Ordering::AcqRel) => return,
            _ => {}
        }
        // A send error means the session was dropped; nobody is left to care.
        let _ = self.tx.send(event);
        self.ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;

    #[test]
    fn wakeups_are_coalesced_until_cleared() {
        let (tx, rx) = mpsc::channel();
        let proxy = EventProxy::new(tx, egui::Context::default());
        for _ in 0..3 {
            proxy.send_event(Event::Wakeup);
        }
        assert_eq!(rx.try_iter().count(), 1);

        proxy.clear_wakeup();
        proxy.send_event(Event::Wakeup);
        assert_eq!(rx.try_iter().count(), 1);
    }

    #[test]
    fn other_events_are_not_coalesced() {
        let (tx, rx) = mpsc::channel();
        let proxy = EventProxy::new(tx, egui::Context::default());
        proxy.send_event(Event::Bell);
        proxy.send_event(Event::Bell);
        proxy.send_event(Event::Exit);
        assert_eq!(rx.try_iter().count(), 3);
    }

    #[test]
    fn replies_bypass_the_ui_queue() {
        // Replies go straight to the PTY; the UI never sees them, connected or not.
        let (tx, rx) = mpsc::channel();
        let proxy = EventProxy::new(tx, egui::Context::default());
        proxy.send_event(Event::PtyWrite("\x1b[1;1R".to_owned()));
        assert_eq!(rx.try_iter().count(), 0);
    }
}
