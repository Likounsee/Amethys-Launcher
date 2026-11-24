//! Experimental Amethys desktop UI rendered with GhostEngine and connected to Amethys Core.
pub mod covers;
pub mod core_client;
pub mod data;
pub mod file_dialog;
pub mod gfx;
pub mod pages;
pub mod status;

#[cfg(feature = "window")]
pub mod app;
