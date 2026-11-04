import { create } from "@bufbuild/protobuf";
import { describe, expect, it } from "vitest";

import {
	batchTemplateOf,
	containerExtension,
	containerName,
	emptyPresetForm,
	presetContainerOf,
	presetTemplateOf,
	ruleDraftsOf,
	rulesSummary,
	streamsTemplateOf,
	toPresetSettings,
	toStreamsForm,
	unsavableReason
} from "./presets";
import { StreamType } from "~/generated/muxmultitool/files/v1/files_pb";
import { PresetContainer, PresetSchema, PresetSettingsSchema } from "~/generated/muxmultitool/presets/v1/presets_pb";
import { RuleAction, StreamRulesSchema, UntaggedLanguage } from "~/generated/muxmultitool/streams/v1/streams_pb";
import { emptyForm, type StreamsForm } from "~/features/streams/lib/streams";

const rules = create(StreamRulesSchema, {
	rules: [
		{ action: RuleAction.DROP, streamType: StreamType.AUDIO, match: { case: "language", value: "fre" } },
		{ action: RuleAction.DROP, streamType: StreamType.AUDIO, match: { case: "codec", value: "dts" } },
		{ action: RuleAction.KEEP, streamType: StreamType.UNSPECIFIED, match: { case: "streamIndex", value: 2 } },
		{ action: RuleAction.KEEP, streamType: StreamType.SUBTITLE, match: { case: undefined } }
	],
	untagged: UntaggedLanguage.DROP
});
const english = create(PresetSchema, {
	id: "1",
	name: "English only, drop DTS, MKV",
	settings: { container: PresetContainer.MKV, template: "{name}.{lang}", rules }
});
const typeName = (type: StreamType) => StreamType[type].toLowerCase();

describe("containers", () => {
	it("maps every preset container to its extension and back", () => {
		expect(containerExtension(PresetContainer.MKV)).toBe("mkv");
		expect(containerExtension(PresetContainer.WEBM)).toBe("webm");
		expect(containerExtension(PresetContainer.UNSPECIFIED)).toBe("mkv");
		expect(containerName(PresetContainer.MP4)).toBe("MP4");
		expect(presetContainerOf("mov")).toBe(PresetContainer.MOV);
		expect(presetContainerOf("MP4")).toBe(PresetContainer.MP4);
		expect(presetContainerOf("mka")).toBeUndefined();
	});
});

describe("templates", () => {
	it("adds the container's extension for a batch and {ext} for the Streams form", () => {
		expect(batchTemplateOf({ container: PresetContainer.MP4, template: "{name}.{lang}" })).toBe("{name}.{lang}.mp4");
		expect(streamsTemplateOf({ template: "{name}.{lang}" })).toBe("{name}.{lang}.{ext}");
	});

	it("takes the extension off a Streams template", () => {
		expect(presetTemplateOf("{name}.copy.{ext}", "mkv")).toBe("{name}.copy");
		expect(presetTemplateOf("{name}.MKV", "mkv")).toBe("{name}");
		expect(presetTemplateOf(" {name}.{ext} ", "mkv")).toBe("{name}");
		expect(presetTemplateOf("{name}", "mkv")).toBe("{name}");
		expect(presetTemplateOf(".{ext}", "mkv")).toBe(".{ext}");
	});
});

describe("rules", () => {
	it("turns a rules message into drafts, leaving out a rule without a match", () => {
		expect(ruleDraftsOf(rules)).toEqual([
			{ action: "drop", streamType: StreamType.AUDIO, match: "language", value: "fre" },
			{ action: "drop", streamType: StreamType.AUDIO, match: "codec", value: "dts" },
			{ action: "keep", streamType: StreamType.UNSPECIFIED, match: "streamIndex", value: "2" }
		]);
		expect(ruleDraftsOf(undefined)).toEqual([]);
	});

	it("summarises the rules for the list", () => {
		expect(rulesSummary(english.settings, typeName)).toBe("drop audio fre, drop audio dts, keep stream 2");
		expect(rulesSummary(create(PresetSettingsSchema), typeName)).toBe("");
	});
});

describe("toStreamsForm", () => {
	it("fills the form's rules, untagged, container and template", () => {
		expect(toStreamsForm(english)).toEqual({
			action: "copy",
			videoOnly: false,
			rules: ruleDraftsOf(rules),
			untagged: "drop",
			container: "mkv",
			template: "{name}.{lang}.{ext}"
		});
	});

	it("copes with a preset without settings", () => {
		expect(toStreamsForm({ settings: undefined })).toMatchObject({ rules: [], untagged: "keep", container: "mkv", template: ".{ext}" });
	});
});

describe("toPresetSettings", () => {
	const form: StreamsForm = {
		...emptyForm(),
		container: "mp4",
		template: "{name}.{lang}.{ext}",
		untagged: "drop",
		rules: [
			{ action: "drop", streamType: StreamType.AUDIO, match: "language", value: "fre" },
			{ action: "keep", streamType: StreamType.AUDIO, match: "language", value: "" }
		]
	};

	it("is the inverse of toStreamsForm", () => {
		const settings = toPresetSettings(form);
		expect(settings).toBeDefined();
		expect(settings?.container).toBe(PresetContainer.MP4);
		expect(settings?.template).toBe("{name}.{lang}");
		expect(settings?.rules?.untagged).toBe(UntaggedLanguage.DROP);
		expect(settings?.rules?.rules).toHaveLength(1);
		expect(toStreamsForm({ settings })).toMatchObject({ container: "mp4", template: "{name}.{lang}.{ext}", untagged: "drop" });
	});

	it("refuses what a preset cannot hold, with the reason", () => {
		expect(unsavableReason(form)).toBeUndefined();
		expect(unsavableReason({ ...form, action: "extract" })).toBe("extract");
		expect(unsavableReason({ ...form, videoOnly: true })).toBe("videoOnly");
		expect(unsavableReason({ ...form, container: "mka" })).toBe("container");
		expect(toPresetSettings({ ...form, videoOnly: true })).toBeUndefined();
		expect(toPresetSettings({ ...form, container: "m4a" })).toBeUndefined();
	});

	it("starts a new preset from an empty form", () => {
		expect(toPresetSettings(emptyPresetForm())).toMatchObject({ container: PresetContainer.MKV, template: "{name}" });
	});
});
