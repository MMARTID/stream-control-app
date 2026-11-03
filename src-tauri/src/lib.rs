//! Stream Control: control en directo para OBS.

pub mod control;
pub mod obs;
pub mod preview;
pub mod settings;

mod app;

pub use app::run;
