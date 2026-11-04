import { create } from "@bufbuild/protobuf";
import { describe, expect, it } from "vitest";

import { JOB_LOG_LIMIT, applyEvent, applyFetchedJob, applySnapshot, countJobs, mergeLog, queueEventId, sortJobs } from "./jobQueue";
import {
	type Job,
	JobEventSchema,
	JobSchema,
	JobState,
	ProgressSchema,
	QueueSnapshotSchema,
	WatchQueueResponseSchema
} from "~/generated/muxmultitool/jobs/v1/jobs_pb";

function job(id: string, state: JobState, extra: Partial<Omit<Job, "$typeName" | "$unknown">> = {}): Job {
	return create(JobSchema, { id, name: id, state, command: `ffmpeg -i ${id}`, ...extra });
}

const stateEvent = (id: number, jobId: string, state: JobState, atUnixMs = 0, message?: string) =>
	create(JobEventSchema, {
		id: BigInt(id),
		jobId,
		event: { case: "state", value: { state, atUnixMs: BigInt(atUnixMs), error: message ? { message, ffmpegOutput: "raw" } : undefined } }
	});
const logEvent = (id: number, jobId: string, line: string) =>
	create(JobEventSchema, { id: BigInt(id), jobId, event: { case: "logLine", value: line } });
const progressEvent = (id: number, jobId: string, percent: number) =>
	create(JobEventSchema, { id: BigInt(id), jobId, event: { case: "progress", value: create(ProgressSchema, { percent, speed: 2.5 }) } });

describe("mergeLog", () => {
	it("takes the tail as is when nothing is known yet", () => {
		expect(mergeLog([], ["a", "b"])).toEqual(["a", "b"]);
	});

	it("keeps the overlap once and appends the rest", () => {
		expect(mergeLog(["a", "b", "c"], ["b", "c", "d"])).toEqual(["a", "b", "c", "d"]);
	});

	it("never shortens the log when the tail is already included", () => {
		expect(mergeLog(["a", "b", "c", "d"], ["c", "d"])).toEqual(["a", "b", "c", "d"]);
	});

	it("appends a tail that does not overlap at all", () => {
		expect(mergeLog(["a"], ["x", "y"])).toEqual(["a", "x", "y"]);
	});

	it("stays within the limit", () => {
		const existing = Array.from({ length: JOB_LOG_LIMIT }, (_, i) => `line ${i}`);
		const merged = mergeLog(existing, ["new"]);
		expect(merged).toHaveLength(JOB_LOG_LIMIT);
		expect(merged.at(-1)).toBe("new");
		expect(merged[0]).toBe("line 1");
	});
});

describe("queueEventId", () => {
	it("reads the id from either message kind", () => {
		const snapshot = create(WatchQueueResponseSchema, { message: { case: "snapshot", value: { eventId: 7n } } });
		const event = create(WatchQueueResponseSchema, { message: { case: "event", value: logEvent(9, "a", "x") } });
		expect(queueEventId(snapshot)).toBe(7n);
		expect(queueEventId(event)).toBe(9n);
		expect(queueEventId(create(WatchQueueResponseSchema))).toBe(0n);
	});
});

describe("applySnapshot", () => {
	it("replaces the jobs but keeps the longer log history", () => {
		const known = new Map([["a", job("a", JobState.RUNNING, { log: ["1", "2", "3"] })]]);
		const snapshot = create(QueueSnapshotSchema, {
			eventId: 10n,
			jobs: [job("a", JobState.SUCCEEDED, { log: ["3", "4"] }), job("b", JobState.QUEUED)]
		});

		const next = applySnapshot(known, snapshot);

		expect([...next.keys()]).toEqual(["a", "b"]);
		expect(next.get("a")?.state).toBe(JobState.SUCCEEDED);
		expect(next.get("a")?.log).toEqual(["1", "2", "3", "4"]);
	});

	it("forgets jobs the server no longer has", () => {
		const known = new Map([["gone", job("gone", JobState.SUCCEEDED)]]);
		expect(applySnapshot(known, create(QueueSnapshotSchema, { jobs: [job("a", JobState.QUEUED)] })).has("gone")).toBe(false);
	});
});

describe("applyEvent", () => {
	it("moves a job through its states with the timestamps and the error", () => {
		const jobs = new Map([["a", job("a", JobState.QUEUED)]]);

		expect(applyEvent(jobs, stateEvent(1, "a", JobState.RUNNING, 1000))).toEqual({ unknown: false });
		expect(jobs.get("a")).toMatchObject({ state: JobState.RUNNING, startedUnixMs: 1000n });

		applyEvent(jobs, stateEvent(2, "a", JobState.FAILED, 2000, "disk full"));
		expect(jobs.get("a")).toMatchObject({
			state: JobState.FAILED,
			finishedUnixMs: 2000n,
			error: { message: "disk full", ffmpegOutput: "raw" }
		});
	});

	it("records progress and log lines in order", () => {
		const jobs = new Map([["a", job("a", JobState.RUNNING)]]);
		applyEvent(jobs, progressEvent(1, "a", 42));
		applyEvent(jobs, logEvent(2, "a", "frame=1"));
		applyEvent(jobs, logEvent(3, "a", "frame=2"));

		expect(jobs.get("a")?.progress?.percent).toBe(42);
		expect(jobs.get("a")?.log).toEqual(["frame=1", "frame=2"]);
	});

	it("caps the log at the limit, dropping the oldest lines", () => {
		const jobs = new Map([["a", job("a", JobState.RUNNING, { log: Array.from({ length: JOB_LOG_LIMIT }, (_, i) => `${i}`) })]]);
		applyEvent(jobs, logEvent(1, "a", "last"));

		const log = jobs.get("a")?.log ?? [];
		expect(log).toHaveLength(JOB_LOG_LIMIT);
		expect(log[0]).toBe("1");
		expect(log.at(-1)).toBe("last");
	});

	it("holds a place for a job it has never seen and asks for it", () => {
		const jobs = new Map<string, Job>();

		expect(applyEvent(jobs, stateEvent(1, "new", JobState.QUEUED, 5))).toEqual({ unknown: true });
		expect(jobs.get("new")).toMatchObject({ id: "new", state: JobState.QUEUED, command: "" });

		applyEvent(jobs, logEvent(2, "new", "early line"));
		// The fetched tail already has the line that event 3 brings next: the client's own log wins.
		applyFetchedJob(jobs, job("new", JobState.RUNNING, { log: ["early line", "later"] }));
		applyEvent(jobs, logEvent(3, "new", "later"));
		expect(jobs.get("new")).toMatchObject({ command: "ffmpeg -i new", state: JobState.RUNNING, log: ["early line", "later"] });
	});

	it("never sets a job back to a state the stream has already left", () => {
		const jobs = new Map<string, Job>();
		applyEvent(jobs, stateEvent(1, "fast", JobState.QUEUED, 10));
		applyEvent(jobs, stateEvent(2, "fast", JobState.RUNNING, 11));
		applyEvent(jobs, stateEvent(3, "fast", JobState.FAILED, 12, "permission denied"));
		// The fetch for the unknown job was answered while it still ran
		applyFetchedJob(jobs, job("fast", JobState.RUNNING, { startedUnixMs: 11n }));
		expect(jobs.get("fast")).toMatchObject({
			command: "ffmpeg -i fast",
			state: JobState.FAILED,
			error: { message: "permission denied" },
			startedUnixMs: 11n,
			finishedUnixMs: 12n
		});
	});

	it("takes a fetched job as is when nothing was known", () => {
		const jobs = new Map<string, Job>();
		applyFetchedJob(jobs, job("a", JobState.QUEUED, { log: ["tail"] }));
		expect(jobs.get("a")?.log).toEqual(["tail"]);
	});
});

describe("sortJobs", () => {
	it("lists running, then queued in submission order, then finished newest first", () => {
		const jobs = new Map(
			[
				job("done-old", JobState.SUCCEEDED, { createdUnixMs: 1n, finishedUnixMs: 10n }),
				job("queued-2", JobState.QUEUED, { createdUnixMs: 6n }),
				job("failed-new", JobState.FAILED, { createdUnixMs: 2n, finishedUnixMs: 20n }),
				job("queued-1", JobState.QUEUED, { createdUnixMs: 5n }),
				job("running", JobState.RUNNING, { createdUnixMs: 3n })
			].map((j) => [j.id, j] as const)
		);

		expect(sortJobs(jobs).map((j) => j.id)).toEqual(["running", "queued-1", "queued-2", "failed-new", "done-old"]);
	});
});

describe("countJobs", () => {
	it("counts running, queued and finished jobs", () => {
		const jobs = [
			job("a", JobState.RUNNING),
			job("b", JobState.QUEUED),
			job("c", JobState.QUEUED),
			job("d", JobState.FAILED),
			job("e", JobState.CANCELLED)
		];
		expect(countJobs(jobs)).toEqual({ running: 1, queued: 2, finished: 2 });
	});
});
