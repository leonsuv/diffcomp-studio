// =============================================================================
// dc_app - Library Entry Point
// =============================================================================
// Exposes the main application struct and modules.
// =============================================================================

#![warn(missing_docs)]

//! # dc_app - DiffComp Studio Application
//!
//! The main GUI application for document comparison.

mod app;
mod docking;
pub mod formula;
/// Internationalization (i18n) for English / German
pub mod i18n;
mod panels;
mod theme;

/// Session persistence (save/load)
pub mod persistence;
mod state;
/// Undo/redo system
pub mod undo;
mod widgets;

pub use app::DiffCompApp;
pub use state::{AppState, SessionState};
