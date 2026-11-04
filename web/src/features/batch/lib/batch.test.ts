import { create } from "@bufbuild/protobuf";
import { describe, expect, it } from "vitest";

import {
	type BatchForm,
	emptyBatchForm,
	formProblems,
	planRows,
	progressCounts,
	progressRows,
	splitPatterns,
	toApplyPresetRequest,
	toBatchOperation
} from "./batch";
import { BatchItemSchema } from "~/generated/muxmultitool/batch/v1/batch_pb";
import { EntryKind, StreamType } from "~/generated/muxmultitool/files/v1/files_pb";
import { JobSchema, JobState } from "~/generated/muxmultitool/jobs/v1/jobs_pb";
import { RuleAction, SelectionMode, UntaggedLanguage } from "~/generated/muxmultitool/streams/v1/streams_pb";

const ready: BatchForm = {
	...emptyBatchForm(),
	folder: "inbox",
	folderLabel: "Inbox",
	patterns: "*.mp4, *.mkv",
	outputFolder: "out",
	outputFolderLabel: "out",
	rules: [{ action: "drop", streamType: StreamType.AUDIO, match: "language", value: "fre" }],
	untagged: "drop"
};

describe("splitPatterns", () => {
	it("splits on commas, semicolons and blanks and drops repeats", () => {
		expect(splitPatterns(" *.mp4, *.mkv;*.mp4  *.avi ")).toEqual(["*.mp4", "*.mkv", "*.avi"]);
		expect(splitPatterns("")).toEqual([]);
	});
});

describe("formProblems", () => {
	it("passes a complete form", () => {
		expect(formProblems(ready)).toEqual([]);
	});

	it("names the folder, the patterns and the template", () => {
		expect(formProblems({ ...ready, folder: "", patterns: " ", template: "{name}.{bad}" })).toEqual([
			"folder",
			"patterns",
			{ template: { kind: "unknown", placeholder: "bad" } }
		]);
	});

	it("wants a preset when that is the source, and ignores the template then", () => {
		expect(formProblems({ ...ready, source: "preset", template: "" })).toEqual(["preset"]);
		expect(formProblems({ ...ready, source: "preset", presetId: "1", template: "" })).toEqual([]);
	});
});

describe("toBatchOperation", () => {
	it("sends the rules with the folders, patterns and template", () => {
		const operation = toBatchOperation(ready);
		expect(operation.folder).toBe("inbox");
		expect(operation.patterns).toEqual(["*.mp4", "*.mkv"]);
		expect(operation.outputFolder).toBe("out");
		expect(operation.template).toBe("{name}.mkv");
		expect(operation.mode).toBe(SelectionMode.RULES);
		expect(operation.rules?.untagged).toBe(UntaggedLanguage.DROP);
		expect(operation.rules?.rules[0]?.action).toBe(RuleAction.DROP);
		expect(operation.overwrite).toBe(false);
	});

	it("is a remux without rules and video only with the switch", () => {
		expect(toBatchOperation({ ...ready, rules: [] }).mode).toBe(SelectionMode.ALL);
		expect(toBatchOperation({ ...ready, rules: [] }).rules).toBeUndefined();
		expect(toBatchOperation({ ...ready, videoOnly: true }).mode).toBe(SelectionMode.VIDEO_ONLY);
	});

	it("builds the apply-preset request from the same folders", () => {
		const request = toApplyPresetRequest({ ...ready, source: "preset", presetId: "7", recursive: true, overwrite: true });
		expect(request).toMatchObject({
			presetId: "7",
			folder: "inbox",
			patterns: ["*.mp4", "*.mkv"],
			recursive: true,
			outputFolder: "out",
			overwrite: true
		});
	});
});

describe("planRows", () => {
	const items = [
		create(BatchItemSchema, {
			input: { id: "f1", name: "Film 1.mp4", kind: EntryKind.FILE, path: "Inbox/Film 1.mp4" },
			output: "/media/out/Film 1.mkv",
			outputName: "Film 1.mkv",
			ready: true,
			container: "MKV",
			streams: [
				{ stream: { index: 0, type: StreamType.VIDEO, codec: "h264" }, kept: true, why: "no rule" },
				{ stream: { index: 1, type: StreamType.AUDIO, codec: "dts", language: "fre" }, kept: false, why: "rule: drop audio fre" },
				{ stream: { index: 2, type: StreamType.SUBTITLE, codec: "subrip" }, kept: true, why: "no rule" }
			],
			issues: [{ streamIndex: 2, blocking: false, message: "MKV may not hold this" }],
			warnings: ["the rules drop every audio stream"],
			command: "ffmpeg -i a -map 0:0 -c copy out"
		}),
		create(BatchItemSchema, {
			input: { id: "f2", name: "Film 3.mp4", kind: EntryKind.FILE, path: "Inbox/Film 3.mp4" },
			output: "/media/out/Film 3.mkv",
			outputName: "Film 3.mkv",
			skip: "the output exists; allow overwriting to replace it"
		})
	];

	it("counts the kept streams and the warnings and attaches the issues to their stream", () => {
		const rows = planRows(items);
		expect(rows).toHaveLength(2);
		expect(rows[0]).toMatchObject({
			id: "f1",
			input: "Film 1.mp4",
			outputName: "Film 1.mkv",
			container: "MKV",
			ready: true,
			kept: 2,
			total: 3,
			warnings: 2
		});
		expect(rows[0]?.streams.map((row) => row.kept)).toEqual([true, false, true]);
		expect(rows[0]?.streams[2]?.issues).toMatchObject([{ streamIndex: 2, blocking: false, message: "MKV may not hold this" }]);
		expect(rows[0]?.streams[1]?.why).toBe("rule: drop audio fre");
	});

	it("keeps a skipped file with its reason and no streams", () => {
		const row = planRows(items)[1];
		expect(row).toMatchObject({
			id: "f2",
			ready: false,
			skip: "the output exists; allow overwriting to replace it",
			kept: 0,
			total: 0,
			command: ""
		});
	});
});

describe("progress", () => {
	const submission = {
		jobs: [
			{ id: "j1", name: "Film 1.mkv" },
			{ id: "j2", name: "Film 2.mkv" },
			{ id: "j3", name: "Film 3.mkv" }
		],
		skipped: []
	};
	const jobs = new Map([
		["j1", create(JobSchema, { id: "j1", name: "Film 1.mkv", state: JobState.SUCCEEDED, progress: { percent: 40 } })],
		[
			"j2",
			create(JobSchema, {
				id: "j2",
				name: "",
				state: JobState.FAILED,
				error: { message: "ffmpeg exited with 1", ffmpegOutput: "Invalid data" }
			})
		]
	]);

	it("follows every job, counting an unknown one as queued", () => {
		const rows = progressRows(submission, (id) => jobs.get(id));
		expect(rows).toEqual([
			{ jobId: "j1", name: "Film 1.mkv", state: JobState.SUCCEEDED, percent: 100, error: undefined, ffmpegOutput: undefined },
			{
				jobId: "j2",
				name: "Film 2.mkv",
				state: JobState.FAILED,
				percent: undefined,
				error: "ffmpeg exited with 1",
				ffmpegOutput: "Invalid data"
			},
			{ jobId: "j3", name: "Film 3.mkv", state: JobState.QUEUED }
		]);
	});

	it("counts the states and the share done", () => {
		const rows = progressRows(submission, (id) => jobs.get(id));
		expect(progressCounts(rows)).toEqual({ queued: 1, running: 0, succeeded: 1, failed: 1, cancelled: 0, percent: 67 });
		expect(progressCounts([])).toEqual({ queued: 0, running: 0, succeeded: 0, failed: 0, cancelled: 0, percent: 0 });
	});
});
