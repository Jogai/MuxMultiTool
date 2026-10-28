//! Everything that touches ffmpeg and ffprobe.
//!
//! Operations describe what they want as an [`FfmpegCommand`]; the
//! [`Runner`] is the only place in the workspace that spawns a process. It
//! owns timeouts, cancellation, progress parsing and logging, so every
//! feature gets the same behaviour for free.
//!
//! ```text
//! operations ──FfmpegCommand──▶ Runner ──▶ ffmpeg  ──progress lines──▶ Progress
//!                               Runner ──▶ ffprobe ──JSON───────────▶ ProbeResult
//! ```

pub mod capabilities;
pub mod command;
pub mod operations;
pub mod probe;
pub mod progress;
pub mod runner;
pub mod tools;

pub use capabilities::{Capabilities, CapabilityCache, CapabilityError};
pub use command::FfmpegCommand;
pub use probe::ProbeResult;
pub use progress::Progress;
pub use runner::{Completed, Limits, RunError, Runner};
pub use tools::{Tool, Tools};
