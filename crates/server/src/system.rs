//! `SystemService`: reports on the running server itself.

use std::future::{Future, ready};

use connectrpc::{RequestContext, Response, ServiceRequest, ServiceResult};
use muxmultitool_proto::connect::muxmultitool::system::v1::SystemService;
use muxmultitool_proto::proto::muxmultitool::system::v1::{
	GetHealthRequest, GetHealthResponse, GetVersionRequest, GetVersionResponse, HealthStatus,
};

/// The version reported by `GetVersion`: the workspace version from `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Implements the system service.
#[derive(Debug, Default)]
pub(crate) struct System;

impl SystemService for System {
	// Neither method awaits anything, so they return ready futures instead of
	// being `async fn` (clippy::unused_async_trait_impl).
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
}
