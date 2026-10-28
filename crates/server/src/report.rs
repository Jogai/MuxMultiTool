//! The stream report over RPC: `FilesService/GetStreamReport` and
//! `GetStreamDetails`.
//!
//! The logic is `core::report`; this module resolves the selected ids to
//! paths (on the blocking pool, like the rest of the files service), runs
//! the reports one file at a time, so a wildcard selection does not start
//! one ffprobe per file at once, and maps the results to the wire
//! messages. A file that fails is one [`FileReport`] with an error; only a
//! bad selection fails the call.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use connectrpc::{ConnectError, Response, ServiceResult};
use muxmultitool_core::ffmpeg::probe::CodecType;
use muxmultitool_core::files::{self, Entry, FileId, Kind, MediaRoots};
use muxmultitool_core::report::{
	self, Report, ReportError, ReportOptions, SizeMethod, StreamReport,
};
use muxmultitool_proto::proto::muxmultitool::files::v1::{
	self as pb, FindFilesRequest, GetStreamDetailsRequest, GetStreamDetailsResponse,
	GetStreamReportRequest, GetStreamReportResponse,
};
use tracing::warn;

use crate::AppState;
use crate::files::{connect_error, entry};

/// One selected file, resolved on the blocking pool.
enum Selected {
	/// A media file the report can probe.
	File {
		id: String,
		entry: Entry,
		path: PathBuf,
	},
	/// An id that does not name a readable media file. The entry is there
	/// when the id resolves to something else, such as a folder.
	Failed {
		id: String,
		entry: Option<Entry>,
		message: String,
	},
}

/// `GetStreamReport`.
pub(crate) async fn stream_report(
	state: &Arc<AppState>,
	request: GetStreamReportRequest,
) -> ServiceResult<GetStreamReportResponse> {
	let selected = {
		let state = Arc::clone(state);
		let (ids, selections) = (request.file_ids, request.selections);
		tokio::task::spawn_blocking(move || select(&state.files, ids, selections))
			.await
			.map_err(|error| task_failed(&error))??
	};
	let options = ReportOptions {
		quick: request.quick,
		..ReportOptions::default()
	};
	let mut files = Vec::with_capacity(selected.len());
	for file in selected {
		files.push(match file {
			Selected::File { id, entry, path } => {
				match report::report(&state.runner, &path, &options).await {
					Ok(report) => file_report(id, Some(entry), Ok(report)),
					Err(error) => file_report(id, Some(entry), Err(error)),
				}
			}
			Selected::Failed { id, entry, message } => file_report(
				id,
				entry,
				Err(ReportError {
					message,
					ffprobe_output: String::new(),
				}),
			),
		});
	}
	Ok(Response::new(GetStreamReportResponse {
		files,
		..Default::default()
	}))
}

/// `GetStreamDetails`.
pub(crate) async fn stream_details(
	state: &Arc<AppState>,
	request: GetStreamDetailsRequest,
) -> ServiceResult<GetStreamDetailsResponse> {
	let path = {
		let state = Arc::clone(state);
		let id = FileId::new(request.file_id);
		tokio::task::spawn_blocking(move || state.files.path(&id))
			.await
			.map_err(|error| task_failed(&error))?
			.map_err(connect_error)?
	};
	let details = report::details(
		&state.runner,
		&path,
		request.stream_index,
		&ReportOptions::default(),
	)
	.await
	.map_err(|error| ConnectError::failed_precondition(error.message))?;
	let Some(details) = details else {
		return Err(ConnectError::not_found(format!(
			"the file has no stream with index {}",
			request.stream_index
		)));
	};
	Ok(Response::new(GetStreamDetailsResponse {
		fields: details
			.fields
			.into_iter()
			.map(|(key, value)| pb::DetailField {
				key,
				value,
				..Default::default()
			})
			.collect(),
		json: details.json,
		..Default::default()
	}))
}

/// Resolves the ids, then each selection, in order and without duplicates.
fn select(
	roots: &MediaRoots,
	ids: Vec<String>,
	selections: Vec<FindFilesRequest>,
) -> Result<Vec<Selected>, ConnectError> {
	let mut seen = HashSet::new();
	let mut selected = Vec::new();
	for id in ids {
		if seen.insert(id.clone()) {
			selected.push(resolve(roots, id));
		}
	}
	for selection in selections {
		let folder = FileId::new(selection.folder_id);
		let found = roots
			.find(&folder, &selection.patterns, selection.recursive)
			.map_err(connect_error)?;
		for entry in found {
			let id = entry.id.as_str().to_owned();
			if seen.insert(id.clone()) {
				selected.push(match roots.path(&entry.id) {
					Ok(path) => Selected::File { id, entry, path },
					Err(error) => Selected::Failed {
						id,
						entry: None,
						message: failure_message(&error),
					},
				});
			}
		}
	}
	Ok(selected)
}

/// The file behind one id from the client.
fn resolve(roots: &MediaRoots, id: String) -> Selected {
	let file_id = FileId::new(id.as_str());
	let resolved = roots
		.get(&file_id)
		.and_then(|entry| Ok((entry, roots.path(&file_id)?)));
	match resolved {
		Ok((entry, _)) if entry.kind == Kind::Folder => Selected::Failed {
			id,
			entry: Some(entry),
			message: String::from("a folder, not a file"),
		},
		Ok((entry, path)) => Selected::File { id, entry, path },
		Err(error) => Selected::Failed {
			id,
			entry: None,
			message: failure_message(&error),
		},
	}
}

/// A files error as the report shows it: never a server path.
fn failure_message(error: &files::Error) -> String {
	match error {
		files::Error::Io { path, source } => {
			warn!(path = %path.display(), error = %source, "report file cannot be read");
			String::from("the file could not be read")
		}
		other => other.to_string(),
	}
}

fn task_failed(error: &tokio::task::JoinError) -> ConnectError {
	warn!(%error, "files task failed");
	ConnectError::internal("the files task failed")
}

fn file_report(
	id: String,
	entry: Option<Entry>,
	result: Result<Report, ReportError>,
) -> pb::FileReport {
	let (container, streams, error) = match result {
		Ok(report) => (
			Some(container(&report)),
			report.streams.into_iter().map(stream).collect(),
			None,
		),
		Err(error) => (
			None,
			Vec::new(),
			Some(pb::ReportError {
				message: error.message,
				ffprobe_output: error.ffprobe_output,
				..Default::default()
			}),
		),
	};
	pb::FileReport {
		file_id: id,
		entry: entry.map(self::entry).into(),
		container: container.into(),
		streams,
		error: error.into(),
		..Default::default()
	}
}

fn container(report: &Report) -> pb::ContainerInfo {
	let c = &report.container;
	pb::ContainerInfo {
		format: c.format.clone(),
		format_long_name: c.format_long_name.clone(),
		duration_seconds: c.duration.unwrap_or_default(),
		size_bytes: c.size.unwrap_or_default(),
		size_gb: gigabytes(c.size),
		bit_rate: c.bit_rate.unwrap_or_default(),
		title: c.title.clone().unwrap_or_default(),
		chapter_count: u32::try_from(report.chapters).unwrap_or(u32::MAX),
		..Default::default()
	}
}

pub(crate) fn stream(s: StreamReport) -> pb::StreamSummary {
	pb::StreamSummary {
		index: s.index,
		r#type: match s.kind {
			CodecType::Video => pb::StreamType::STREAM_TYPE_VIDEO,
			CodecType::Audio => pb::StreamType::STREAM_TYPE_AUDIO,
			CodecType::Subtitle => pb::StreamType::STREAM_TYPE_SUBTITLE,
			CodecType::Data => pb::StreamType::STREAM_TYPE_DATA,
			CodecType::Attachment => pb::StreamType::STREAM_TYPE_ATTACHMENT,
			CodecType::Unknown => pb::StreamType::STREAM_TYPE_UNSPECIFIED,
		}
		.into(),
		codec: s.codec,
		codec_long_name: s.codec_long_name.unwrap_or_default(),
		profile: s.profile.unwrap_or_default(),
		language: s.language.unwrap_or_default(),
		title: s.title.unwrap_or_default(),
		size_bytes: s.size.unwrap_or_default(),
		size_gb: gigabytes(s.size),
		size_method: match s.size_method {
			SizeMethod::Unknown => pb::SizeMethod::SIZE_METHOD_UNSPECIFIED,
			SizeMethod::Packets => pb::SizeMethod::SIZE_METHOD_PACKETS,
			SizeMethod::Tags => pb::SizeMethod::SIZE_METHOD_TAGS,
			SizeMethod::Extradata => pb::SizeMethod::SIZE_METHOD_EXTRADATA,
			SizeMethod::Estimated => pb::SizeMethod::SIZE_METHOD_ESTIMATED,
		}
		.into(),
		duration_seconds: s.duration.unwrap_or_default(),
		bit_rate: s.bit_rate.unwrap_or_default(),
		width: s.width.unwrap_or_default(),
		height: s.height.unwrap_or_default(),
		frame_rate: s.frame_rate.unwrap_or_default(),
		pix_fmt: s.pix_fmt.unwrap_or_default(),
		channels: s.channels.unwrap_or_default(),
		channel_layout: s.channel_layout.unwrap_or_default(),
		sample_rate: s.sample_rate.unwrap_or_default(),
		flags: s.flags,
		..Default::default()
	}
}

/// Bytes as decimal gigabytes, `0.0` when unknown.
fn gigabytes(bytes: Option<u64>) -> f64 {
	#[expect(clippy::cast_precision_loss, reason = "a size for display")]
	bytes.map_or(0.0, |b| b as f64 / 1e9)
}
