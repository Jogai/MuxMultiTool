import { create } from "@bufbuild/protobuf";

import { type ExtractStreamsOperation, ExtractStreamsOperationSchema } from "~/generated/muxmultitool/extract/v1/extract_pb";
import { type Entry, type StreamSummary, StreamType } from "~/generated/muxmultitool/files/v1/files_pb";
import {
	type PreviewStreamsResponse,
	RuleAction,
	type SelectStreamsOperation,
	SelectStreamsOperationSchema,
	SelectionMode,
	type StreamRule,
	type StreamRules,
	StreamRuleSchema,
	StreamRulesSchema,
	UntaggedLanguage
} from "~/generated/muxmultitool/streams/v1/streams_pb";
import { languagesOf, padNumber, parentName, renderTemplate as renderFields, stem, utcDate } from "~/features/batch/lib/template";

export { stem };

/**
 * The state of the Streams screen (sketch 02) and the operations it turns into. Rules apply to every
 * file; per-file ticks override the rules for that file only ("rules first, ticks second"). The
 * backend has no mode for both at once, so a file with overrides is submitted as a plain KEEP of the
 * streams the rules resolved to, flipped by the ticks (`effectiveOperation`).
 */

/** What the screen does with the selection: copy the kept streams to a new container, or extract them to one file each. */
export type StreamsAction = "copy" | "extract";

export type RuleMatchKind = "language" | "codec" | "streamIndex";

/** One filter rule as the form holds it; `value` is the language, the codec or the stream index. */
export interface RuleDraft {
	action: "drop" | "keep";
	/** `StreamType.UNSPECIFIED` means every stream. */
	streamType: StreamType;
	match: RuleMatchKind;
	value: string;
}

/** The per-file overrides: the streams whose tick was flipped by hand, keyed by file id. */
export type Overrides = ReadonlyMap<string, ReadonlySet<number>>;

export interface StreamsForm {
	action: StreamsAction;
	/** Keep only video (mode VIDEO_ONLY); the rules are then ignored. */
	videoOnly: boolean;
	rules: readonly RuleDraft[];
	untagged: "keep" | "drop";
	/** The extension that picks the target container, e.g. `mkv`. */
	container: string;
	/** An absolute folder inside a media root; empty puts the output next to the input. */
	folder: string;
	/** The output file name with `{name}` (the input's name without extension) and `{ext}`. */
	template: string;
	overrides: Overrides;
}

export const DEFAULT_TEMPLATE = "{name}.copy.{ext}";

export function emptyForm(): StreamsForm {
	return {
		action: "copy",
		videoOnly: false,
		rules: [],
		untagged: "keep",
		container: "mkv",
		folder: "",
		template: DEFAULT_TEMPLATE,
		overrides: new Map()
	};
}

/** The target containers the backend knows, by the extension that selects each (`Container::from_extension`). */
export const CONTAINERS: readonly { extension: string; name: string }[] = [
	{ extension: "mkv", name: "MKV" },
	{ extension: "mp4", name: "MP4" },
	{ extension: "mov", name: "MOV" },
	{ extension: "webm", name: "WebM" },
	{ extension: "mka", name: "MKA (Matroska audio)" },
	{ extension: "m4a", name: "M4A (MP4 audio)" }
];

/** The stream types a rule can be limited to, "any" first. */
export const RULE_STREAM_TYPES: readonly StreamType[] = [
	StreamType.UNSPECIFIED,
	StreamType.VIDEO,
	StreamType.AUDIO,
	StreamType.SUBTITLE,
	StreamType.ATTACHMENT,
	StreamType.DATA
];

/** The one-click rules of the sketch's quick chips; "video only" is a mode, not a rule. */
export const QUICK_RULES: readonly { name: string; rules: readonly RuleDraft[] }[] = [
	{ name: "englishAudio", rules: [{ action: "keep", streamType: StreamType.AUDIO, match: "language", value: "eng" }] },
	{
		name: "dropFrench",
		rules: [
			{ action: "drop", streamType: StreamType.AUDIO, match: "language", value: "fre" },
			{ action: "drop", streamType: StreamType.SUBTITLE, match: "language", value: "fre" }
		]
	},
	{ name: "dropDts", rules: [{ action: "drop", streamType: StreamType.AUDIO, match: "codec", value: "dts" }] }
];

/** Whether a rule has what the backend needs: a value, and for a stream index a whole number. */
export function isRuleComplete(rule: RuleDraft): boolean {
	const value = rule.value.trim();
	if (value === "") {
		return false;
	}
	return rule.match !== "streamIndex" || /^\d+$/.test(value);
}

/** `rules` with `rule` added, unless an equal rule is already there. */
export function addRule(rules: readonly RuleDraft[], rule: RuleDraft): RuleDraft[] {
	const added: RuleDraft = { ...rule, value: rule.value.trim() };
	return rules.some((existing) => sameRule(existing, added)) ? [...rules] : [...rules, added];
}

export function sameRule(a: RuleDraft, b: RuleDraft): boolean {
	return a.action === b.action && a.streamType === b.streamType && a.match === b.match && a.value.toLowerCase() === b.value.toLowerCase();
}

/** `items` with the element at `from` moved to `to`; out-of-range moves leave it as it is. */
export function move<T>(items: readonly T[], from: number, to: number): T[] {
	const next = [...items];
	if (from === to || from < 0 || to < 0 || from >= next.length || to >= next.length) {
		return next;
	}
	const [item] = next.splice(from, 1);
	next.splice(to, 0, item as T);
	return next;
}

/** The short name of a rule, as the backend's "why" column spells it: `drop audio fre`, `keep eng`, `drop stream 2`. */
export function ruleName(rule: RuleDraft, typeName: (type: StreamType) => string): string {
	const parts: string[] = [rule.action];
	if (rule.streamType !== StreamType.UNSPECIFIED) {
		parts.push(typeName(rule.streamType));
	}
	parts.push(rule.match === "streamIndex" ? `stream ${rule.value.trim()}` : rule.value.trim());
	return parts.join(" ");
}

function ruleMatch(rule: RuleDraft): StreamRule["match"] {
	const value = rule.value.trim();
	switch (rule.match) {
		case "language":
			return { case: "language", value };
		case "codec":
			return { case: "codec", value };
		default:
			return { case: "streamIndex", value: Number(value) };
	}
}

/** The rules message, leaving out rules that are not complete yet. */
export function toStreamRules(form: Pick<StreamsForm, "rules" | "untagged">): StreamRules {
	return create(StreamRulesSchema, {
		rules: form.rules.filter(isRuleComplete).map((rule) =>
			create(StreamRuleSchema, {
				action: rule.action === "drop" ? RuleAction.DROP : RuleAction.KEEP,
				streamType: rule.streamType,
				match: ruleMatch(rule)
			})
		),
		untagged: form.untagged === "drop" ? UntaggedLanguage.DROP : UntaggedLanguage.KEEP
	});
}

/**
 * The fields of the output name besides `{name}` and `{ext}`, which the screen knows per file: the
 * kept audio languages once the preview is in, the file's number in the selection and its folder.
 */
export interface NameFields {
	lang?: string;
	n?: string;
	parent?: string;
	date?: string;
}

/**
 * The output file name of the template for an input, e.g. `{name}.eng.{ext}` → `Movie.eng.mkv`. Here
 * `{ext}` is the container's extension (a batch keeps the input's); `{lang}` is `und` until the
 * preview says which audio streams stay.
 */
export function renderTemplate(template: string, inputName: string, extension: string, fields: NameFields = {}): string {
	return renderFields(template.trim(), {
		name: stem(inputName),
		ext: extension,
		lang: fields.lang ?? "und",
		date: fields.date ?? utcDate(new Date()),
		n: fields.n ?? "1",
		parent: fields.parent ?? ""
	}).trim();
}

/** `{lang}` of a previewed file: the languages of the audio streams it keeps, `und` when none is tagged. */
export function keptLanguages(preview: Pick<PreviewStreamsResponse, "streams">): string {
	return languagesOf(
		preview.streams.flatMap((result) =>
			result.stream ? [{ kept: result.kept, isAudio: result.stream.type === StreamType.AUDIO, language: result.stream.language }] : []
		)
	);
}

/** The name fields of the file at `index` of `count` in the selection, with its languages once previewed. */
export function nameFields(
	file: Pick<Entry, "path">,
	index: number,
	count: number,
	preview?: Pick<PreviewStreamsResponse, "streams">
): NameFields {
	return { lang: preview ? keptLanguages(preview) : undefined, n: padNumber(index, count), parent: parentName(file.path) };
}

/** The output the operation names: the bare name next to the input, or the name inside `folder`. */
export function outputPath(folder: string, fileName: string): string {
	const trimmed = folder.trim();
	if (trimmed === "") {
		return fileName;
	}
	return `${trimmed.replace(/\/+$/, "")}/${fileName}`;
}

/** The mode the form's rules and switches resolve to before any tick is applied. */
export function baseMode(form: Pick<StreamsForm, "videoOnly" | "rules">): SelectionMode {
	if (form.videoOnly) {
		return SelectionMode.VIDEO_ONLY;
	}
	return form.rules.some(isRuleComplete) ? SelectionMode.RULES : SelectionMode.ALL;
}

/** The copy operation of the rules and switches alone: what the preview resolves per file. */
export function baseOperation(form: StreamsForm, fileId: string, fileName: string, fields?: NameFields): SelectStreamsOperation {
	const mode = baseMode(form);
	return create(SelectStreamsOperationSchema, {
		input: fileId,
		output: outputPath(form.folder, renderTemplate(form.template, fileName, form.container, fields)),
		mode,
		rules: mode === SelectionMode.RULES ? toStreamRules(form) : undefined
	});
}

/** The indexes the file's overrides flipped; empty when it has none. */
export function overridesOf(overrides: Overrides, fileId: string): ReadonlySet<number> {
	return overrides.get(fileId) ?? new Set();
}

/** `overrides` with the stream's tick flipped for the file. */
export function toggleOverride(overrides: Overrides, fileId: string, index: number): Map<string, ReadonlySet<number>> {
	const next = new Map(overrides);
	const file = new Set(overridesOf(overrides, fileId));
	if (!file.delete(index)) {
		file.add(index);
	}
	if (file.size === 0) {
		next.delete(fileId);
	} else {
		next.set(fileId, file);
	}
	return next;
}

/** `overrides` without the file's ticks: back to what the rules say. */
export function clearOverrides(overrides: Overrides, fileId: string): Map<string, ReadonlySet<number>> {
	const next = new Map(overrides);
	next.delete(fileId);
	return next;
}

/** `overrides` with only the listed files left, for when the selection changes. */
export function pruneOverrides(overrides: Overrides, fileIds: readonly string[]): Map<string, ReadonlySet<number>> {
	const keep = new Set(fileIds);
	return new Map([...overrides].filter(([fileId]) => keep.has(fileId)));
}

/** One row of the selection table: the stream, whether it ends up in the output and why. */
export interface SelectionRow {
	stream: StreamSummary;
	kept: boolean;
	/** The backend's reason; empty when the tick was flipped by hand (the UI words that). */
	why: string;
	/** True when the row's tick was flipped by hand, so it no longer follows the rules. */
	overridden: boolean;
}

/** A row of the selection table: the selection row plus what the issues and the extraction say about it. */
export interface TableRow extends SelectionRow {
	/** The file the stream is written to, when extracting. */
	output?: string;
	/** Problems the target container has with the stream, or why it has no file of its own. */
	issues: readonly { blocking: boolean; message: string }[];
}

/** The rows of a file: the base preview's decisions with the file's overrides applied. */
export function selectionRows(preview: PreviewStreamsResponse, overrides: ReadonlySet<number>): SelectionRow[] {
	return preview.streams.flatMap((result) => {
		if (!result.stream) {
			return [];
		}
		const overridden = overrides.has(result.stream.index);
		return [{ stream: result.stream, kept: overridden ? !result.kept : result.kept, why: overridden ? "" : result.why, overridden }];
	});
}

/** The indexes the file ends up keeping: what the rules resolved to, flipped by the ticks. */
export function effectiveKept(preview: PreviewStreamsResponse, overrides: ReadonlySet<number>): number[] {
	return selectionRows(preview, overrides)
		.filter((row) => row.kept)
		.map((row) => row.stream.index);
}

/**
 * The operation to submit for a file: the base operation while nothing is ticked by hand, otherwise a
 * KEEP of the effective streams. Needs the base preview to know what the rules resolved to.
 */
export function effectiveOperation(
	form: StreamsForm,
	fileId: string,
	fileName: string,
	preview: PreviewStreamsResponse | undefined,
	fields?: NameFields
): SelectStreamsOperation {
	const base = baseOperation(form, fileId, fileName, fields);
	const overrides = overridesOf(form.overrides, fileId);
	if (overrides.size === 0 || !preview) {
		return base;
	}
	return create(SelectStreamsOperationSchema, {
		input: base.input,
		output: base.output,
		mode: SelectionMode.KEEP,
		streamIndexes: effectiveKept(preview, overrides)
	});
}

/** Whether a copy of the file needs its own KEEP preview: it has ticks of its own and the base preview is in. */
export function needsEffectivePreview(form: StreamsForm, fileId: string, preview: PreviewStreamsResponse | undefined): boolean {
	return overridesOf(form.overrides, fileId).size > 0 && preview !== undefined;
}

/**
 * The extract operation for a file. Extraction starts from every stream and knows no rules; an
 * unticked stream is left out (mode DROP), so a stream without a file format is skipped with a note
 * and never blocks the job.
 */
export function extractOperation(form: Pick<StreamsForm, "folder" | "overrides">, fileId: string): ExtractStreamsOperation {
	const overrides = [...overridesOf(form.overrides, fileId)].toSorted((a, b) => a - b);
	return create(ExtractStreamsOperationSchema, {
		input: fileId,
		outputFolder: form.folder.trim(),
		mode: overrides.length > 0 ? SelectionMode.DROP : SelectionMode.ALL,
		streamIndexes: overrides
	});
}

/** The totals of the result line: `5 of 8 streams · 24.49 GB (−0.53 GB)`. */
export interface Totals {
	kept: number;
	total: number;
	keptBytes: number;
	droppedBytes: number;
}

export function totals(rows: readonly SelectionRow[]): Totals {
	let keptBytes = 0;
	let droppedBytes = 0;
	let kept = 0;
	for (const row of rows) {
		const bytes = Number(row.stream.sizeBytes);
		if (row.kept) {
			kept++;
			keptBytes += bytes;
		} else {
			droppedBytes += bytes;
		}
	}
	return { kept, total: rows.length, keptBytes, droppedBytes };
}

/** The link to a job on the Jobs screen. */
export function jobLink(jobId: string): { path: string; query: { job: string } } {
	return { path: "/jobs", query: { job: jobId } };
}
