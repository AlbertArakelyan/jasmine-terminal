//! Terminal backend: owns the PTY and the `alacritty_terminal` state, independent of egui
//! painting so it can be driven headlessly in integration tests.

mod event_proxy;
mod session;
mod snapshot;
