//! `PresetsService`: saved presets (container, filter rules, output name)
//! and their application.
//!
//! The logic is `muxmultitool_core::presets`. A preset is applied by
//! building the operation a client would otherwise enter by hand:
//! `ApplyPresetToBatch` returns a `BatchOperation`, `ApplyPresetToFile` a
//! `SelectStreamsOperation` whose output name comes from the same
//! `batch::plan` a batch runs, so the plan (and the names) are those of
//! the hand-entered settings.

use std::sync::Arc;
use std::time::SystemTime;

use connectrpc::{ConnectError, RequestContext, Response, ServiceRequest, ServiceResult};
use muxmultitool_core::batch::{self, Destination, Existing, Input, Outcome, utc_date};
use muxmultitool_core::ffmpeg::probe::CodecType;
use muxmultitool_core::presets::{self, Preset, Settings};
use muxmultitool_core::streams::Container;
use muxmultitool_core::streams::rules::{Action, Matcher, Rule, Rules, Untagged};
use muxmultitool_proto::connect::muxmultitool::presets::v1::PresetsService;
use muxmultitool_proto::proto::muxmultitool::batch::v1::BatchOperation;
use muxmultitool_proto::proto::muxmultitool::files::v1::StreamType;
use muxmultitool_proto::proto::muxmultitool::presets::v1::{
	self as proto, ApplyPresetToBatchRequest, ApplyPresetToBatchResponse, ApplyPresetToFileRequest,
	ApplyPresetToFileResponse, CreatePresetRequest, CreatePresetResponse, DeletePresetRequest,
	DeletePresetResponse, ListPresetsRequest, ListPresetsResponse, PresetContainer,
	RenamePresetRequest, RenamePresetResponse, UpdatePresetRequest, UpdatePresetResponse,
};
use muxmultitool_proto::proto::muxmultitool::streams::v1::__buffa::oneof::stream_rule::Match;
use muxmultitool_proto::proto::muxmultitool::streams::v1::{
	RuleAction, SelectStreamsOperation, SelectionMode, StreamRule, StreamRules, UntaggedLanguage,
};
use tracing::warn;

use crate::AppState;
use crate::jobs::Jobs;
use crate::streams::{file_name, rules};

/// Implements the presets service.
#[derive(Debug)]
pub(crate) struct Presets {
	jobs: Jobs,
}

impl Presets {
	pub(crate) const fn new(state: Arc<AppState>) -> Self {
		Self {
			jobs: Jobs { state },
		}
	}

	async fn preset(&self, id: &str) -> Result<Preset, ConnectError> {
		self.jobs
			.state
			.presets
			.get(id)
			.await
			.map_err(|e| preset_error(&e))
	}
}

impl PresetsService for Presets {
	async fn list_presets(
		&self,
		_ctx: RequestContext,
		_request: ServiceRequest<'_, ListPresetsRequest>,
	) -> ServiceResult<ListPresetsResponse> {
		let presets = self
			.jobs
			.state
			.presets
			.list()
			.await
			.map_err(|e| preset_error(&e))?;
		Ok(Response::new(ListPresetsResponse {
			presets: presets.iter().map(preset_message).collect(),
			..Default::default()
		}))
	}

	async fn create_preset(
		&self,
		_ctx: RequestContext,
		request: ServiceRequest<'_, CreatePresetRequest>,
	) -> ServiceResult<CreatePresetResponse> {
		let request = request.to_owned_message();
		let settings = settings_from(request.settings.as_option())?;
		let preset = self
			.jobs
			.state
			.presets
			.create(&request.name, settings)
			.await
			.map_err(|e| preset_error(&e))?;
		Ok(Response::new(CreatePresetResponse {
			preset: Some(preset_message(&preset)).into(),
			..Default::default()
		}))
	}

	async fn update_preset(
		&self,
		_ctx: RequestContext,
		request: ServiceRequest<'_, UpdatePresetRequest>,
	) -> ServiceResult<UpdatePresetResponse> {
		let request = request.to_owned_message();
		let settings = settings_from(request.settings.as_option())?;
		let preset = self
			.jobs
			.state
			.presets
			.update(&request.id, settings)
			.await
			.map_err(|e| preset_error(&e))?;
		Ok(Response::new(UpdatePresetResponse {
			preset: Some(preset_message(&preset)).into(),
			..Default::default()
		}))
	}

	async fn rename_preset(
		&self,
		_ctx: RequestContext,
		request: ServiceRequest<'_, RenamePresetRequest>,
	) -> ServiceResult<RenamePresetResponse> {
		let request = request.to_owned_message();
		let preset = self
			.jobs
			.state
			.presets
			.rename(&request.id, &request.name)
			.await
			.map_err(|e| preset_error(&e))?;
		Ok(Response::new(RenamePresetResponse {
			preset: Some(preset_message(&preset)).into(),
			..Default::default()
		}))
	}

	async fn delete_preset(
		&self,
		_ctx: RequestContext,
		request: ServiceRequest<'_, DeletePresetRequest>,
	) -> ServiceResult<DeletePresetResponse> {
		let request = request.to_owned_message();
		self.jobs
			.state
			.presets
			.delete(&request.id)
			.await
			.map_err(|e| preset_error(&e))?;
		Ok(Response::new(DeletePresetResponse::default()))
	}

	async fn apply_preset_to_file(
		&self,
		_ctx: RequestContext,
		request: ServiceRequest<'_, ApplyPresetToFileRequest>,
	) -> ServiceResult<ApplyPresetToFileResponse> {
		let request = request.to_owned_message();
		let preset = self.preset(&request.preset_id).await?;
		let path = self.jobs.stream_input(&request.input).await?;
		let probe = self.jobs.probe_stream_input(&path).await?;
		// The name comes from the batch planner with this one file, so it
		// is the name a batch of one would write.
		let batch = preset
			.settings
			.batch(Destination::NextToInput, Existing::Overwrite)
			.map_err(|e| preset_error(&e))?;
		let input = Input {
			path,
			probe: Ok(probe),
		};
		let items = batch::plan(&batch, &[input], &utc_date(SystemTime::now()));
		let item = items
			.first()
			.ok_or_else(|| ConnectError::internal("the plan is empty"))?;
		let Some(output) = item.output.as_deref() else {
			let reason = match &item.outcome {
				Outcome::Skipped(skip) => skip.to_string(),
				Outcome::Ready(_) => String::new(),
			};
			return Err(ConnectError::failed_precondition(format!(
				"the preset cannot be applied to this file: {reason}"
			)));
		};
		Ok(Response::new(ApplyPresetToFileResponse {
			operation: Some(SelectStreamsOperation {
				input: request.input,
				output: file_name(output),
				mode: SelectionMode::Rules.into(),
				rules: Some(rules_message(&preset.settings.rules)).into(),
				..Default::default()
			})
			.into(),
			..Default::default()
		}))
	}

	async fn apply_preset_to_batch(
		&self,
		_ctx: RequestContext,
		request: ServiceRequest<'_, ApplyPresetToBatchRequest>,
	) -> ServiceResult<ApplyPresetToBatchResponse> {
		let request = request.to_owned_message();
		let preset = self.preset(&request.preset_id).await?;
		let template = preset
			.settings
			.output_template()
			.map_err(|e| preset_error(&e))?;
		Ok(Response::new(ApplyPresetToBatchResponse {
			operation: Some(BatchOperation {
				folder: request.folder,
				patterns: request.patterns,
				recursive: request.recursive,
				output_folder: request.output_folder,
				template: template.source().to_owned(),
				overwrite: request.overwrite,
				mode: SelectionMode::Rules.into(),
				rules: Some(rules_message(&preset.settings.rules)).into(),
				..Default::default()
			})
			.into(),
			..Default::default()
		}))
	}
}

/// The status a store error maps to.
fn preset_error(error: &presets::Error) -> ConnectError {
	use presets::Error;
	match error {
		Error::NotFound(_) => ConnectError::not_found(error.to_string()),
		Error::DuplicateName(_) => ConnectError::already_exists(error.to_string()),
		Error::Invalid(_) => ConnectError::invalid_argument(error.to_string()),
		Error::Unreadable { .. } => ConnectError::failed_precondition(error.to_string()),
		Error::Io { .. } => {
			warn!(%error, "cannot store the presets");
			ConnectError::internal("the presets could not be saved")
		}
	}
}

fn settings_from(settings: Option<&proto::PresetSettings>) -> Result<Settings, ConnectError> {
	let Some(settings) = settings else {
		return Err(ConnectError::invalid_argument("settings are required"));
	};
	let container = match settings.container.as_known() {
		Some(PresetContainer::Mkv) => Container::Mkv,
		Some(PresetContainer::Mp4) => Container::Mp4,
		Some(PresetContainer::Mov) => Container::Mov,
		Some(PresetContainer::Webm) => Container::Webm,
		Some(PresetContainer::Unspecified) => {
			return Err(ConnectError::invalid_argument("a container is required"));
		}
		None => {
			return Err(ConnectError::invalid_argument(format!(
				"unknown container {}",
				settings.container.to_i32()
			)));
		}
	};
	let rules = match settings.rules.as_option() {
		Some(r) => rules(Some(r))?,
		None => Rules::default(),
	};
	Ok(Settings {
		container,
		template: settings.template.clone(),
		rules,
	})
}

fn preset_message(preset: &Preset) -> proto::Preset {
	let container = match preset.settings.container {
		Container::Mkv => PresetContainer::Mkv,
		Container::Mp4 => PresetContainer::Mp4,
		Container::Mov => PresetContainer::Mov,
		Container::Webm => PresetContainer::Webm,
	};
	proto::Preset {
		id: preset.id.clone(),
		name: preset.name.clone(),
		settings: Some(proto::PresetSettings {
			container: container.into(),
			template: preset.settings.template.clone(),
			rules: Some(rules_message(&preset.settings.rules)).into(),
			..Default::default()
		})
		.into(),
		..Default::default()
	}
}

/// The inverse of `streams::rules`.
fn rules_message(rules: &Rules) -> StreamRules {
	StreamRules {
		rules: rules.rules.iter().map(rule_message).collect(),
		untagged: match rules.untagged {
			Untagged::Keep => UntaggedLanguage::Keep,
			Untagged::Drop => UntaggedLanguage::Drop,
		}
		.into(),
		..Default::default()
	}
}

fn rule_message(rule: &Rule) -> StreamRule {
	StreamRule {
		action: match rule.action {
			Action::Drop => RuleAction::Drop,
			Action::Keep => RuleAction::Keep,
		}
		.into(),
		stream_type: match rule.stream_type {
			None | Some(CodecType::Unknown) => StreamType::Unspecified,
			Some(CodecType::Video) => StreamType::Video,
			Some(CodecType::Audio) => StreamType::Audio,
			Some(CodecType::Subtitle) => StreamType::Subtitle,
			Some(CodecType::Data) => StreamType::Data,
			Some(CodecType::Attachment) => StreamType::Attachment,
		}
		.into(),
		r#match: Some(match &rule.matcher {
			Matcher::Language(v) => Match::Language(v.clone()),
			Matcher::Codec(v) => Match::Codec(v.clone()),
			Matcher::Stream(i) => Match::StreamIndex(*i),
		}),
		..Default::default()
	}
}
