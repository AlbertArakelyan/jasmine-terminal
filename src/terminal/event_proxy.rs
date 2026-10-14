//! `EventListener` impl that forwards terminal events to the UI thread and requests a repaint,
//! so the UI only redraws when the PTY has produced something.
