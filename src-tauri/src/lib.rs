//! Stream Control: control en directo para OBS.

pub mod avatar;
pub mod control;
pub mod lights;
pub mod obs;
pub mod panel;
pub mod preview;
pub mod settings;

mod app;

pub use app::run;
