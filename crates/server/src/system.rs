//! `SystemService`: reports on the running server itself.

use std::future::{Future, ready};
use std::sync::Arc;

use connectrpc::{ConnectError, RequestContext, Response, ServiceRequest, ServiceResult};
use muxmultitool_core::ffmpeg::capabilities::{self as caps, CodecKind as CoreKind};
use muxmultitool_core::settings;
use muxmultitool_proto::connect::muxmultitool::system::v1::SystemService;
use muxmultitool_proto::proto::muxmultitool::system::v1::{
	Capabilities, CapabilitiesStatus, Codec, CodecKind, Filter, Format, GetCapabilitiesRequest,
	GetCapabilitiesResponse, GetHealthRequest, GetHealthResponse, GetSettingsRequest,
	GetSettingsResponse, GetVersionRequest, GetVersionResponse, HealthStatus, Settings, ThemeMode,
	ThemeSetting, UpdateSettingsRequest, UpdateSettingsResponse,
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

	async fn get_settings(
		&self,
		_ctx: RequestContext,
		_request: ServiceRequest<'_, GetSettingsRequest>,
	) -> ServiceResult<GetSettingsResponse> {
		let settings = self.state.settings.get().await;
		Ok(Response::new(GetSettingsResponse {
			settings: Some(settings_to_proto(settings)).into(),
			..Default::default()
		}))
	}

	async fn update_settings(
		&self,
		_ctx: RequestContext,
		request: ServiceRequest<'_, UpdateSettingsRequest>,
	) -> ServiceResult<UpdateSettingsResponse> {
		let patch = request
			.to_owned_message()
			.settings
			.into_option()
			.map(settings_from_proto)
			.unwrap_or_default();
		let stored = self.state.settings.update(patch).await.map_err(|error| {
			warn!(%error, "cannot store the settings");
			ConnectError::internal("the settings could not be saved")
		})?;
		Ok(Response::new(UpdateSettingsResponse {
			settings: Some(settings_to_proto(stored)).into(),
			..Default::default()
		}))
	}
}

fn settings_to_proto(settings: settings::Settings) -> Settings {
	Settings {
		theme: settings
			.theme
			.map(|theme| ThemeSetting {
				family: theme.family,
				mode: match theme.mode {
					settings::ThemeMode::Single => ThemeMode::THEME_MODE_SINGLE,
					settings::ThemeMode::Auto => ThemeMode::THEME_MODE_AUTO,
				}
				.into(),
				variant: theme.variant,
				light_variant: theme.light_variant,
				dark_variant: theme.dark_variant,
				..Default::default()
			})
			.into(),
		..Default::default()
	}
}

/// An unknown or unspecified mode counts as `Single`: the frontend only
/// switches to auto when the request says so.
fn settings_from_proto(settings: Settings) -> settings::Settings {
	settings::Settings {
		theme: settings
			.theme
			.into_option()
			.map(|theme| settings::ThemeSetting {
				family: theme.family,
				mode: match theme.mode.as_known() {
					Some(ThemeMode::THEME_MODE_AUTO) => settings::ThemeMode::Auto,
					_ => settings::ThemeMode::Single,
				},
				variant: theme.variant,
				light_variant: theme.light_variant,
				dark_variant: theme.dark_variant,
			}),
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
