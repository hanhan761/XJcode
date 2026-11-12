pub mod api;
pub mod auth;
pub mod capabilities;
pub mod catalog_watch;
mod diagnostics;
pub mod doctor;
pub mod images;
pub mod login_status;
#[cfg(windows)]
pub mod ipc;
pub mod protocol;
pub mod setup;
pub mod support;
pub mod provider;
pub mod updates;
pub mod lifecycle;
