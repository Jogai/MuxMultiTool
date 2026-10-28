//! `SplitService`: the segments of a split by duration, size or count.
//!
//! The calculation is `muxmultitool_core::cut::split`; this module resolves
//! the input, probes it, reads the keyframes of the whole file and maps the
//! result to the messages. The export is the ordinary cut job, so the
//! response carries a ready `CutOperation`.

use std::sync::Arc;

use connectrpc::{ConnectError, RequestContext, Response, ServiceRequest, ServiceResult};
use muxmultitool_core::cut::split::{self, Input, Mode, Warning};
use muxmultitool_core::cut::whole_file_window;
use muxmultitool_proto::connect::muxmultitool::cut::v1::SplitService;
use muxmultitool_proto::proto::muxmultitool::cut::v1::{
	CutExport, CutMode, CutOperation, CutSegment, PlanSplitRequest, PlanSplitResponse, SegmentMode,
	SplitPart, plan_split_request::Mode as ModeMessage,
};

use crate::AppState;
use crate::jobs::Jobs;

/// Implements the split service on the cut service's probing and keyframe
/// reading.
#[derive(Debug)]
pub(crate) struct Split {
	jobs: Jobs,
}

impl Split {
	pub(crate) const fn new(state: Arc<AppState>) -> Self {
		Self {
			jobs: Jobs { state },
		}
	}
}

impl SplitService for Split {
	async fn plan_split(
		&self,
		_ctx: RequestContext,
		request: ServiceRequest<'_, PlanSplitRequest>,
	) -> ServiceResult<PlanSplitResponse> {
		let request = request.to_owned_message();
		let mode = match request.mode {
			Some(ModeMessage::MaxDurationSeconds(seconds)) => Mode::Duration(seconds),
			Some(ModeMessage::MaxSizeBytes(bytes)) => Mode::Size(bytes),
			Some(ModeMessage::Parts(count)) => Mode::Count(count),
			None => return Err(ConnectError::invalid_argument("a split mode is required")),
		};
		let input = self.jobs.cut_input(&request.file_id)?;
		let (probe, source) = self.jobs.cut_source(&input).await?;
		let size_bytes = probe
			.format
			.size
			.or_else(|| std::fs::metadata(&input).ok().map(|m| m.len()))
			.filter(|size| *size > 0);
		let keyframes = match source.video_stream {
			None => None,
			Some(stream) => {
				let found = self
					.jobs
					.keyframes(&input, stream, &whole_file_window(&source))
					.await?;
				Some(
					found
						.iter()
						.map(|k| k.pts - source.start_time)
						.collect::<Vec<_>>(),
				)
			}
		};
		let result = split::calculate(
			mode,
			&Input {
				duration: source.duration,
				size_bytes,
				keyframes: keyframes.as_deref(),
			},
		)
		.map_err(|error| match error {
			split::Error::NoDuration | split::Error::NoSize => {
				ConnectError::failed_precondition(error.to_string())
			}
			split::Error::InvalidLimit | split::Error::TooManyParts => {
				ConnectError::invalid_argument(error.to_string())
			}
		})?;
		let operation = CutOperation {
			input: request.file_id.clone(),
			segments: result
				.parts
				.iter()
				.map(|p| CutSegment {
					start_seconds: p.segment.start,
					end_seconds: p.segment.end,
					..Default::default()
				})
				.collect(),
			segment_mode: SegmentMode::SEGMENT_MODE_KEEP.into(),
			cut_mode: CutMode::CUT_MODE_KEYFRAME.into(),
			export: CutExport::CUT_EXPORT_SEPARATE_FILES.into(),
			output: request.output,
			..Default::default()
		};
		Ok(Response::new(PlanSplitResponse {
			parts: result
				.parts
				.iter()
				.map(|p| SplitPart {
					number: u32::try_from(p.number).unwrap_or(u32::MAX),
					start_seconds: p.segment.start,
					end_seconds: p.segment.end,
					estimated_bytes: p.estimated_bytes,
					..Default::default()
				})
				.collect(),
			warnings: result.warnings.iter().map(|w| warning_text(*w)).collect(),
			operation: Some(operation).into(),
			duration_seconds: source.duration,
			size_bytes: size_bytes.unwrap_or(0),
			..Default::default()
		}))
	}
}

fn warning_text(warning: Warning) -> String {
	match warning {
		Warning::PartTooLong { number } => {
			format!("part {number} has no keyframe within the limit, so it is longer than asked")
		}
		Warning::FewerParts { wanted, got } => {
			format!("the file has too few keyframes for {wanted} parts; it makes {got}")
		}
	}
}
