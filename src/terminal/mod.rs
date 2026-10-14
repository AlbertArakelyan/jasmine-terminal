//! Terminal backend: owns the PTY and the `alacritty_terminal` state, independent of egui
//! painting so it can be driven headlessly in integration tests.

pub mod event_proxy;
pub mod session;
mod snapshot;
