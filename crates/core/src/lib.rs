//! MuxMultiTool core library.
//!
//! Holds all application logic: ffprobe parsing, stream selection rules,
//! cutting and the job queue. The server and desktop crates only wrap it.

pub mod batch;
pub mod combine;
pub mod crop;
pub mod cut;
pub mod extract;
pub mod ffmpeg;
pub mod files;
pub mod fs;
pub mod gif;
pub mod jobs;
pub mod merge;
pub mod preview;
pub mod report;
pub mod settings;
pub mod snapshot;
pub mod streams;
pub mod timeline;
