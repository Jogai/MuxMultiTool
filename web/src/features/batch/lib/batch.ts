import { create } from "@bufbuild/protobuf";

import { checkTemplate, type TemplateProblem } from "./template";
import { type BatchItem, type BatchOperation, BatchOperationSchema } from "~/generated/muxmultitool/batch/v1/batch_pb";
import { type Job, JobState } from "~/generated/muxmultitool/jobs/v1/jobs_pb";
import { type ApplyPresetToBatchRequest, ApplyPresetToBatchRequestSchema } from "~/generated/muxmultitool/presets/v1/presets_pb";
import { SelectionMode } from "~/generated/muxmultitool/streams/v1/streams_pb";
import { baseMode, type RuleDraft, type TableRow, toStreamRules } from "~/features/streams/lib/streams";

/**
 * The state of the Batch screen (sketch 09) and what it turns into: the three steps Input, Operation
 * and Output as one form, the `BatchOperation` the dry run and the submit send, the rows of the dry
 * run's table and the per-file progress of a submitted batch.
 */

/** Where the operation comes from: the screen's own rules, or a saved preset the server turns into one. */
export type OperationSource = "rules" | "preset";

export interface BatchForm {
	/** The folder's id from the picker; empty until one is chosen. */
	folder: string;
	/** The folder as the user knows it: its path under the media root, or the absolute path on desktop. */
	folderLabel: string;
	/** The wildcards, comma- or space-separated: `*.mp4, *.mkv`. */
	patterns: string;
	recursive: boolean;
	source: OperationSource;
	/** The preset applied, when the source is a preset. */
	presetId: string;
	/** Keep only video (mode VIDEO_ONLY); the rules are then ignored. */
	videoOnly: boolean;
	rules: readonly RuleDraft[];
	untagged: "keep" | "drop";
	/** The output folder's id; empty puts every output next to its input. */
	outputFolder: string;
	outputFolderLabel: string;
	/** The output name template, e.g. `{name}.mkv`. */
	template: string;
	/** Replace an output that exists instead of skipping the file. */
	overwrite: boolean;
}

export const DEFAULT_BATCH_TEMPLATE = "{name}.mkv";
export const DEFAULT_PATTERNS = "*.mp4";

export function emptyBatchForm(): BatchForm {
	return {
		folder: "",
		folderLabel: "",
		patterns: DEFAULT_PATTERNS,
		recursive: false,
		source: "rules",
		presetId: "",
		videoOnly: false,
		rules: [],
		untagged: "keep",
		outputFolder: "",
		outputFolderLabel: "",
		template: DEFAULT_BATCH_TEMPLATE,
		overwrite: false
	};
}

/** The wildcards of the pattern field, trimmed, without blanks or repeats. */
export function splitPatterns(text: string): string[] {
	const patterns: string[] = [];
	for (const pattern of text.split(/[\s,;]+/)) {
		if (pattern !== "" && !patterns.includes(pattern)) {
			patterns.push(pattern);
		}
	}
	return patterns;
}

/** What keeps the form from being planned, in the order the screen lists them. */
export type FormProblem = "folder" | "patterns" | "preset" | { template: TemplateProblem };

export function formProblems(form: BatchForm): FormProblem[] {
	const problems: FormProblem[] = [];
	if (form.folder === "") {
		problems.push("folder");
	}
	if (splitPatterns(form.patterns).length === 0) {
		problems.push("patterns");
	}
	if (form.source === "preset") {
		if (form.presetId === "") {
			problems.push("preset");
		}
	} else {
		const template = checkTemplate(form.template.trim());
		if (template) {
			problems.push({ template });
		}
	}
	return problems;
}

/** The operation the screen's own rules give; the preset source asks the server instead (`toApplyPresetRequest`). */
export function toBatchOperation(form: BatchForm): BatchOperation {
	const mode = baseMode(form);
	return create(BatchOperationSchema, {
		folder: form.folder,
		patterns: splitPatterns(form.patterns),
		recursive: form.recursive,
		outputFolder: form.outputFolder,
		template: form.template.trim(),
		overwrite: form.overwrite,
		mode,
		rules: mode === SelectionMode.RULES ? toStreamRules(form) : undefined
	});
}

/** The request that turns the chosen preset into the batch operation for the form's folders. */
export function toApplyPresetRequest(form: BatchForm): ApplyPresetToBatchRequest {
	return create(ApplyPresetToBatchRequestSchema, {
		presetId: form.presetId,
		folder: form.folder,
		patterns: splitPatterns(form.patterns),
		recursive: form.recursive,
		outputFolder: form.outputFolder,
		overwrite: form.overwrite
	});
}

/** One file of the dry run as the table shows it. */
export interface PlanRow {
	/** The input's id, the row's key. */
	id: string;
	input: string;
	/** The input's path for display, relative to its media root. */
	inputPath: string;
	outputName: string;
	output: string;
	container: string;
	ready: boolean;
	skip: string;
	overwrites: boolean;
	kept: number;
	total: number;
	/** Container warnings and selection warnings, counted for the badge. */
	warnings: number;
	command: string;
	/** The streams for the expanded row, as the Streams table draws them. */
	streams: TableRow[];
	issues: readonly { blocking: boolean; message: string }[];
	selectionWarnings: readonly string[];
}

/** The dry run's rows, in the server's order (by path). */
export function planRows(items: readonly BatchItem[]): PlanRow[] {
	return items.map((item, index) => {
		const issues = new Map<number, { blocking: boolean; message: string }[]>();
		for (const issue of item.issues) {
			issues.set(issue.streamIndex, [...(issues.get(issue.streamIndex) ?? []), issue]);
		}
		const streams = item.streams.flatMap((result) =>
			result.stream
				? [{ stream: result.stream, kept: result.kept, why: result.why, overridden: false, issues: issues.get(result.stream.index) ?? [] }]
				: []
		);
		return {
			id: item.input?.id ?? String(index),
			input: item.input?.name ?? "",
			inputPath: item.input?.path ?? "",
			outputName: item.outputName,
			output: item.output,
			container: item.container,
			ready: item.ready,
			skip: item.skip,
			overwrites: item.overwrites,
			kept: streams.filter((row) => row.kept).length,
			total: streams.length,
			warnings: item.issues.length + item.warnings.length,
			command: item.command,
			streams,
			issues: item.issues,
			selectionWarnings: item.warnings
		};
	});
}

/** A submitted batch: the jobs in file order and the files that got none. */
export interface BatchSubmission {
	jobs: readonly { id: string; name: string }[];
	skipped: readonly BatchItem[];
}

/** One file of a running batch. */
export interface ProgressRow {
	jobId: string;
	name: string;
	state: JobState;
	/** The share done: 100 once succeeded, the last report otherwise. */
	percent?: number;
	/** The plain-language error of a failed job, and ffmpeg's last lines. */
	error?: string;
	ffmpegOutput?: string;
}

/**
 * The per-file status of a submitted batch, from the live queue: a job the queue does not know yet
 * (the stream has not caught up) counts as queued.
 */
export function progressRows(submission: BatchSubmission, lookup: (id: string) => Job | undefined): ProgressRow[] {
	return submission.jobs.map((submitted) => {
		const job = lookup(submitted.id);
		if (!job) {
			return { jobId: submitted.id, name: submitted.name, state: JobState.QUEUED };
		}
		return {
			jobId: job.id,
			name: job.name || submitted.name,
			state: job.state,
			percent: job.state === JobState.SUCCEEDED ? 100 : job.progress?.percent,
			error: job.state === JobState.FAILED ? job.error?.message || undefined : undefined,
			ffmpegOutput: job.state === JobState.FAILED ? job.error?.ffmpegOutput || undefined : undefined
		};
	});
}

/** How many files of the batch are in each state. */
export interface ProgressCounts {
	queued: number;
	running: number;
	succeeded: number;
	failed: number;
	cancelled: number;
	/** The share of the files that reached a final state, 0 to 100. */
	percent: number;
}

export function progressCounts(rows: readonly ProgressRow[]): ProgressCounts {
	const counts = { queued: 0, running: 0, succeeded: 0, failed: 0, cancelled: 0 };
	for (const row of rows) {
		switch (row.state) {
			case JobState.RUNNING:
				counts.running += 1;
				break;
			case JobState.SUCCEEDED:
				counts.succeeded += 1;
				break;
			case JobState.FAILED:
				counts.failed += 1;
				break;
			case JobState.CANCELLED:
				counts.cancelled += 1;
				break;
			default:
				counts.queued += 1;
		}
	}
	const done = counts.succeeded + counts.failed + counts.cancelled;
	return { ...counts, percent: rows.length === 0 ? 0 : Math.round((done / rows.length) * 100) };
}
