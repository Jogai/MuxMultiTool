//! `SystemService`: reports on the running server itself.

use std::future::{Future, ready};
use std::sync::Arc;

use connectrpc::{RequestContext, Response, ServiceRequest, ServiceResult};
use muxmultitool_core::ffmpeg::capabilities::{self as caps, CodecKind as CoreKind};
use muxmultitool_proto::connect::muxmultitool::system::v1::SystemService;
use muxmultitool_proto::proto::muxmultitool::system::v1::{
	Capabilities, CapabilitiesStatus, Codec, CodecKind, Filter, Format, GetCapabilitiesRequest,
	GetCapabilitiesResponse, GetHealthRequest, GetHealthResponse, GetVersionRequest,
	GetVersionResponse, HealthStatus,
};
use tracing::warn;

use crate::AppState;

/// The version reported by `GetVersion`: the workspace version from `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Implements the system service.
#[derive(Debug)]
pub(crate) struct System {
	state: Arc<AppState>,
}

impl System {
	pub(crate) const fn new(state: Arc<AppState>) -> Self {
		Self { state }
	}
}

impl SystemService for System {
	// The health and version methods await nothing, so they return ready
	// futures instead of being `async fn` (clippy::unused_async_trait_impl).
	fn get_health(
		&self,
		_ctx: RequestContext,
		_request: ServiceRequest<'_, GetHealthRequest>,
	) -> impl Future<Output = ServiceResult<GetHealthResponse>> {
		// Answering at all means the server is up. Whether ffmpeg works is a
		// capability question, which gets its own method later.
		ready(Ok(Response::new(GetHealthResponse {
			status: HealthStatus::HEALTH_STATUS_OK.into(),
			..Default::default()
		})))
	}

	fn get_version(
		&self,
		_ctx: RequestContext,
		_request: ServiceRequest<'_, GetVersionRequest>,
	) -> impl Future<Output = ServiceResult<GetVersionResponse>> {
		ready(Ok(Response::new(GetVersionResponse {
			version: VERSION.to_owned(),
			..Default::default()
		})))
	}

	async fn get_capabilities(
		&self,
		_ctx: RequestContext,
		_request: ServiceRequest<'_, GetCapabilitiesRequest>,
	) -> ServiceResult<GetCapabilitiesResponse> {
		let response = match self.state.capabilities.get(&self.state.runner).await {
			Ok(found) => GetCapabilitiesResponse {
				status: CapabilitiesStatus::CAPABILITIES_STATUS_AVAILABLE.into(),
				capabilities: Some(to_proto(&found)).into(),
				..Default::default()
			},
			Err(error) => {
				warn!(%error, "ffmpeg capabilities unavailable");
				GetCapabilitiesResponse {
					status: CapabilitiesStatus::CAPABILITIES_STATUS_UNAVAILABLE.into(),
					error: error.to_string(),
					..Default::default()
				}
			}
		};
		Ok(Response::new(response))
	}
}

fn to_proto(found: &caps::Capabilities) -> Capabilities {
	let codec = |c: &caps::Codec| Codec {
		name: c.name.clone(),
		kind: match c.kind {
			CoreKind::Video => CodecKind::CODEC_KIND_VIDEO,
			CoreKind::Audio => CodecKind::CODEC_KIND_AUDIO,
			CoreKind::Subtitle => CodecKind::CODEC_KIND_SUBTITLE,
			CoreKind::Other => CodecKind::CODEC_KIND_OTHER,
		}
		.into(),
		description: c.description.clone(),
		..Default::default()
	};
	Capabilities {
		version: found.version.clone(),
		encoders: found.encoders.iter().map(codec).collect(),
		decoders: found.decoders.iter().map(codec).collect(),
		formats: found
			.formats
			.iter()
			.map(|f| Format {
				name: f.name.clone(),
				description: f.description.clone(),
				demux: f.demux,
				mux: f.mux,
				..Default::default()
			})
			.collect(),
		filters: found
			.filters
			.iter()
			.map(|f| Filter {
				name: f.name.clone(),
				description: f.description.clone(),
				timeline: f.timeline,
				..Default::default()
			})
			.collect(),
		hwaccels: found.hwaccels.clone(),
		..Default::default()
	}
}
