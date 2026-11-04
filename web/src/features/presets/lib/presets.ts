import { create } from "@bufbuild/protobuf";

import { StreamType } from "~/generated/muxmultitool/files/v1/files_pb";
import { type Preset, PresetContainer, type PresetSettings, PresetSettingsSchema } from "~/generated/muxmultitool/presets/v1/presets_pb";
import { RuleAction, type StreamRules, UntaggedLanguage } from "~/generated/muxmultitool/streams/v1/streams_pb";
import { type RuleDraft, type StreamsForm, toStreamRules } from "~/features/streams/lib/streams";

/**
 * A preset is a target container, an output name template and stream rules (sketch 09). This module
 * turns one into what the screens edit (the Streams form's rules, container and template; the batch
 * template) and the Streams form back into a preset's settings, so "Save as preset" and "Apply" are
 * exact inverses of each other.
 */

/** The containers a preset can hold, by the extension the Streams form uses for each. */
export const PRESET_CONTAINERS: readonly { container: PresetContainer; extension: string; name: string }[] = [
	{ container: PresetContainer.MKV, extension: "mkv", name: "MKV" },
	{ container: PresetContainer.MP4, extension: "mp4", name: "MP4" },
	{ container: PresetContainer.MOV, extension: "mov", name: "MOV" },
	{ container: PresetContainer.WEBM, extension: "webm", name: "WebM" }
];

/** The extension a preset's container adds to its template; `mkv` for an unspecified one, as the server does. */
export function containerExtension(container: PresetContainer): string {
	return PRESET_CONTAINERS.find((entry) => entry.container === container)?.extension ?? "mkv";
}

export function containerName(container: PresetContainer): string {
	return PRESET_CONTAINERS.find((entry) => entry.container === container)?.name ?? "MKV";
}

/** The preset container of a Streams form extension; `undefined` for one presets cannot hold (`mka`, `m4a`). */
export function presetContainerOf(extension: string): PresetContainer | undefined {
	return PRESET_CONTAINERS.find((entry) => entry.extension === extension.toLowerCase())?.container;
}

/** The batch template of a preset: its name template plus the container's extension, as the server renders it. */
export function batchTemplateOf(settings: Pick<PresetSettings, "template" | "container">): string {
	return `${settings.template}.${containerExtension(settings.container)}`;
}

/** The Streams form template of a preset: the name template plus `{ext}`, which the form fills with its container. */
export function streamsTemplateOf(settings: Pick<PresetSettings, "template">): string {
	return `${settings.template}.{ext}`;
}

/**
 * A preset's name template from the Streams form's: the trailing `.{ext}` or the container's own
 * extension is taken off, since the preset's container adds it back.
 */
export function presetTemplateOf(streamsTemplate: string, containerExtension: string): string {
	const trimmed = streamsTemplate.trim();
	for (const suffix of [".{ext}", `.${containerExtension}`]) {
		if (trimmed.toLowerCase().endsWith(suffix.toLowerCase()) && trimmed.length > suffix.length) {
			return trimmed.slice(0, -suffix.length);
		}
	}
	return trimmed;
}

/** The form's rule drafts of a rules message; a rule without a match is left out. */
export function ruleDraftsOf(rules: StreamRules | undefined): RuleDraft[] {
	return (rules?.rules ?? []).flatMap((rule) => {
		if (rule.match.case === undefined) {
			return [];
		}
		return [
			{
				action: rule.action === RuleAction.KEEP ? "keep" : "drop",
				streamType: rule.streamType,
				match: rule.match.case,
				value: String(rule.match.value)
			} satisfies RuleDraft
		];
	});
}

/** The part of the Streams form a preset sets; the rest (the files' ticks, the folder) is left as it is. */
export type PresetFormPart = Pick<StreamsForm, "action" | "videoOnly" | "rules" | "untagged" | "container" | "template">;

/** What applying the preset puts into the Streams form. */
export function toStreamsForm(preset: Pick<Preset, "settings">): PresetFormPart {
	const settings = preset.settings ?? create(PresetSettingsSchema);
	return {
		action: "copy",
		videoOnly: false,
		rules: ruleDraftsOf(settings.rules),
		untagged: settings.rules?.untagged === UntaggedLanguage.DROP ? "drop" : "keep",
		container: containerExtension(settings.container),
		template: streamsTemplateOf(settings)
	};
}

/** Why the Streams form cannot be saved as a preset, or `undefined` when it can. */
export type UnsavableReason = "extract" | "videoOnly" | "container";

export function unsavableReason(form: Pick<StreamsForm, "action" | "videoOnly" | "container">): UnsavableReason | undefined {
	if (form.action === "extract") {
		return "extract";
	}
	if (form.videoOnly) {
		return "videoOnly";
	}
	return presetContainerOf(form.container) === undefined ? "container" : undefined;
}

/** The preset settings of the Streams form; `undefined` when the form holds something a preset cannot. */
export function toPresetSettings(
	form: Pick<StreamsForm, "action" | "videoOnly" | "container" | "template" | "rules" | "untagged">
): PresetSettings | undefined {
	const container = presetContainerOf(form.container);
	if (container === undefined || unsavableReason(form) !== undefined) {
		return undefined;
	}
	return create(PresetSettingsSchema, {
		container,
		template: presetTemplateOf(form.template, form.container),
		rules: toStreamRules(form)
	});
}

/** A fresh preset's settings for the "New" dialog: MKV, `{name}`, no rules. */
export function emptyPresetForm(): PresetFormPart {
	return { action: "copy", videoOnly: false, rules: [], untagged: "keep", container: "mkv", template: "{name}.{ext}" };
}

/** The short description of a preset's rules for the list: `drop audio fre, keep audio eng`; empty without rules. */
export function rulesSummary(settings: PresetSettings | undefined, typeName: (type: StreamType) => string): string {
	return ruleDraftsOf(settings?.rules)
		.map((rule) => {
			const parts: string[] = [rule.action];
			if (rule.streamType !== StreamType.UNSPECIFIED) {
				parts.push(typeName(rule.streamType));
			}
			parts.push(rule.match === "streamIndex" ? `stream ${rule.value}` : rule.value);
			return parts.join(" ");
		})
		.join(", ");
}
