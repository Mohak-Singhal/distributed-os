//! Platform provider abstractions.

pub mod clipboard;
/// Notifications provider abstraction.
pub mod notifications;
/// Terminal provider abstraction.
pub mod terminal;
/// File operations provider abstraction.
pub mod file;

pub use clipboard::ClipboardProvider;
