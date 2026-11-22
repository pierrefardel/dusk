//! Cœur de Dusk : détection des écrans et contrôle matériel de la luminosité.
//!
//! - Écrans externes : DDC/CI via `IOAVService` (Apple Silicon).
//! - Écran intégré et écrans Apple : framework privé `DisplayServices`.

pub mod ambient;
mod builtin;
mod cg;
mod config;
mod dim;
pub mod ddc;
mod display;
mod engine;
mod iokit;
pub mod panel;
pub mod quirks;
pub mod solar;
mod sync;

pub use cg::{display_under_cursor, set_display_enabled};
pub use config::{Config, Solar};
pub use dim::DIM_MIN;
pub use display::{list_displays, Backend, Display};
pub use engine::{Control, DisplayState, Engine, SleepingDisplay, SolarPhase, SolarStatus};
pub use panel::{Choice, Panel};
pub use sync::Curve;

#[derive(Debug)]
pub enum Error {
    Unsupported(&'static str),
    Ddc(String),
    Io(std::io::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Unsupported(s) => write!(f, "non supporté : {s}"),
            Error::Ddc(s) => write!(f, "erreur DDC : {s}"),
            Error::Io(e) => write!(f, "erreur E/S : {e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
