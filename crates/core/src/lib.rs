//! MuxMultiTool core library.
//!
//! Holds all application logic: ffprobe parsing, stream selection rules,
//! cutting and the job queue. The server and desktop crates only wrap it.

pub mod ffmpeg;
pub mod jobs;
