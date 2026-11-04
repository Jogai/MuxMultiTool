import { create } from "@bufbuild/protobuf";
import { describe, expect, it } from "vitest";

import {
	addRule,
	baseMode,
	baseOperation,
	clearOverrides,
	effectiveKept,
	effectiveOperation,
	emptyForm,
	extractOperation,
	isRuleComplete,
	keptLanguages,
	move,
	nameFields,
	outputPath,
	pruneOverrides,
	renderTemplate,
	type RuleDraft,
	ruleName,
	selectionRows,
	stem,
	toggleOverride,
	toStreamRules,
	totals
} from "./streams";
import { StreamType } from "~/generated/muxmultitool/files/v1/files_pb";
import { PreviewStreamsResponseSchema, RuleAction, SelectionMode, UntaggedLanguage } from "~/generated/muxmultitool/streams/v1/streams_pb";

const dropFrench: RuleDraft = { action: "drop", streamType: StreamType.AUDIO, match: "language", value: "fre" };
const keepEnglish: RuleDraft = { action: "keep", streamType: StreamType.UNSPECIFIED, match: "language", value: " eng " };
const dropStream: RuleDraft = { action: "drop", streamType: StreamType.UNSPECIFIED, match: "streamIndex", value: "2" };

const preview = create(PreviewStreamsResponseSchema, {
	streams: [
		{ stream: { index: 0, type: StreamType.VIDEO, codec: "hevc", sizeBytes: 100n }, kept: true, why: "no rule applies" },
		{
			stream: { index: 1, type: StreamType.AUDIO, codec: "dts", language: "fre", sizeBytes: 20n },
			kept: false,
			why: "rule: drop audio fre"
		},
		{ stream: { index: 2, type: StreamType.SUBTITLE, codec: "subrip", sizeBytes: 1n }, kept: true, why: "no rule applies" }
	]
});

describe("rules", () => {
	it("knows when a rule is complete", () => {
		expect(isRuleComplete(dropFrench)).toBe(true);
		expect(isRuleComplete({ ...dropFrench, value: "  " })).toBe(false);
		expect(isRuleComplete(dropStream)).toBe(true);
		expect(isRuleComplete({ ...dropStream, value: "two" })).toBe(false);
	});

	it("adds a rule once, trimmed", () => {
		const rules = addRule(addRule([], keepEnglish), { ...keepEnglish, value: "ENG" });
		expect(rules).toEqual([{ ...keepEnglish, value: "eng" }]);
	});

	it("names a rule like the backend's why column", () => {
		const typeName = (type: StreamType) => StreamType[type].toLowerCase();
		expect(ruleName(dropFrench, typeName)).toBe("drop audio fre");
		expect(ruleName(keepEnglish, typeName)).toBe("keep eng");
		expect(ruleName(dropStream, typeName)).toBe("drop stream 2");
	});

	it("builds the rules message and leaves incomplete rules out", () => {
		const rules = toStreamRules({ rules: [dropFrench, { ...dropFrench, value: "" }, keepEnglish, dropStream], untagged: "drop" });
		expect(rules.untagged).toBe(UntaggedLanguage.DROP);
		expect(rules.rules.map((rule) => [rule.action, rule.streamType, rule.match])).toEqual([
			[RuleAction.DROP, StreamType.AUDIO, { case: "language", value: "fre" }],
			[RuleAction.KEEP, StreamType.UNSPECIFIED, { case: "language", value: "eng" }],
			[RuleAction.DROP, StreamType.UNSPECIFIED, { case: "streamIndex", value: 2 }]
		]);
	});

	it("moves items within range only", () => {
		expect(move([1, 2, 3], 0, 2)).toEqual([2, 3, 1]);
		expect(move([1, 2, 3], 2, 5)).toEqual([1, 2, 3]);
	});
});

describe("output names", () => {
	it("renders the template with the input's stem and the container's extension", () => {
		expect(stem("Movie (2024).mkv")).toBe("Movie (2024)");
		expect(stem(".hidden")).toBe(".hidden");
		expect(renderTemplate("{name}.eng.{ext}", "Movie (2024).mkv", "mkv")).toBe("Movie (2024).eng.mkv");
		expect(outputPath("", "a.mkv")).toBe("a.mkv");
		expect(outputPath("/media/out/", "a.mkv")).toBe("/media/out/a.mkv");
	});

	it("renders the batch placeholders with the file's fields, und for the languages until known", () => {
		expect(renderTemplate("{n}-{parent}-{name}.{lang}.{ext}", "Movie.mp4", "mkv")).toBe("1--Movie.und.mkv");
		expect(renderTemplate("{n}-{parent}-{name}.{lang}.{ext}", "Movie.mp4", "mkv", { n: "03", parent: "Films", lang: "eng" })).toBe(
			"03-Films-Movie.eng.mkv"
		);
		expect(renderTemplate("{name}.{date}.{ext}", "Movie.mp4", "mkv", { date: "2026-11-04" })).toBe("Movie.2026-11-04.mkv");
	});

	it("takes the languages from the kept audio streams and the number from the selection", () => {
		const preview = create(PreviewStreamsResponseSchema, {
			streams: [
				{ stream: { index: 0, type: StreamType.VIDEO, language: "jpn" }, kept: true },
				{ stream: { index: 1, type: StreamType.AUDIO, language: "eng" }, kept: true },
				{ stream: { index: 2, type: StreamType.AUDIO, language: "fre" }, kept: false },
				{ stream: { index: 3, type: StreamType.AUDIO, language: "ger" }, kept: true }
			]
		});
		expect(keptLanguages(preview)).toBe("eng-ger");
		expect(keptLanguages(create(PreviewStreamsResponseSchema))).toBe("und");
		expect(nameFields({ path: "Films/2024/Movie.mp4" }, 2, 12, preview)).toEqual({ lang: "eng-ger", n: "03", parent: "2024" });
		expect(nameFields({ path: "Movie.mp4" }, 0, 1)).toEqual({ lang: undefined, n: "1", parent: "" });
	});
});

describe("operations", () => {
	it("picks the mode from the switches and the rules", () => {
		expect(baseMode({ videoOnly: false, rules: [] })).toBe(SelectionMode.ALL);
		expect(baseMode({ videoOnly: false, rules: [{ ...dropFrench, value: "" }] })).toBe(SelectionMode.ALL);
		expect(baseMode({ videoOnly: false, rules: [dropFrench] })).toBe(SelectionMode.RULES);
		expect(baseMode({ videoOnly: true, rules: [dropFrench] })).toBe(SelectionMode.VIDEO_ONLY);
	});

	it("builds the base operation with the rules only in mode RULES", () => {
		const form = { ...emptyForm(), rules: [dropFrench], folder: "/media/out" };
		const operation = baseOperation(form, "id-a", "Movie.mkv");
		expect(operation.input).toBe("id-a");
		expect(operation.output).toBe("/media/out/Movie.copy.mkv");
		expect(operation.mode).toBe(SelectionMode.RULES);
		expect(operation.rules?.rules).toHaveLength(1);
		expect(baseOperation(emptyForm(), "id-a", "Movie.mkv").rules).toBeUndefined();
	});

	it("applies the ticks to the rules' resolution and submits a KEEP", () => {
		let overrides = toggleOverride(new Map(), "a", 1);
		overrides = toggleOverride(overrides, "a", 2);
		const rows = selectionRows(preview, overrides.get("a") ?? new Set());
		expect(rows.map((row) => [row.stream.index, row.kept, row.why, row.overridden])).toEqual([
			[0, true, "no rule applies", false],
			[1, true, "", true],
			[2, false, "", true]
		]);
		expect(totals(rows)).toEqual({ kept: 2, total: 3, keptBytes: 120, droppedBytes: 1 });
		expect(effectiveKept(preview, overrides.get("a") ?? new Set())).toEqual([0, 1]);

		const form = { ...emptyForm(), rules: [dropFrench], overrides };
		const operation = effectiveOperation(form, "a", "Movie.mkv", preview);
		expect(operation.mode).toBe(SelectionMode.KEEP);
		expect(operation.streamIndexes).toEqual([0, 1]);
		expect(operation.rules).toBeUndefined();
		// Without ticks, or before the base preview is in, the base operation is what runs
		expect(effectiveOperation(form, "b", "Other.mkv", undefined).mode).toBe(SelectionMode.RULES);
		expect(effectiveOperation(form, "a", "Movie.mkv", undefined).mode).toBe(SelectionMode.RULES);
	});

	it("toggles, clears and prunes overrides", () => {
		const once = toggleOverride(new Map(), "a", 3);
		expect(once.get("a")).toEqual(new Set([3]));
		expect(toggleOverride(once, "a", 3).has("a")).toBe(false);
		expect(clearOverrides(once, "a").size).toBe(0);
		expect(pruneOverrides(new Map([...once, ["b", new Set([1])]]), ["b"]).has("a")).toBe(false);
	});

	it("extracts everything, or everything but the unticked streams", () => {
		expect(extractOperation({ folder: " /media/out ", overrides: new Map() }, "a")).toMatchObject({
			input: "a",
			outputFolder: "/media/out",
			mode: SelectionMode.ALL,
			streamIndexes: []
		});
		const overrides = toggleOverride(toggleOverride(new Map(), "a", 4), "a", 1);
		expect(extractOperation({ folder: "", overrides }, "a")).toMatchObject({ mode: SelectionMode.DROP, streamIndexes: [1, 4] });
	});
});
