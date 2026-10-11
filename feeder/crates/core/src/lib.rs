//! Feeder core: state model, driver client, control API, OSC sender and logging.
//!
//! The CLI and the GUI are thin front ends over [`Engine`]. Every change goes
//! through a text command (see `docs/control.md`) so that the two front ends,
//! the control pipe and scenario files share one code path.

pub mod command;
pub mod config;
pub mod control;
mod control_client;
pub mod driver_client;
pub mod engine;
pub mod logging;
pub mod osc;
pub mod presets;
pub mod scenario;
mod status;
pub mod win32;
pub mod world;

pub use command::{Command, CommandError, DeviceRef};
pub use config::Config;
pub use engine::Engine;
pub use world::World;
