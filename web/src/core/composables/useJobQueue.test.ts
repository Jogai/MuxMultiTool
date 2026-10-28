import { create } from "@bufbuild/protobuf";
import { type CallOptions, type Client, Code, ConnectError } from "@connectrpc/connect";
import { describe, expect, it, vi } from "vitest";

import { useJobQueue } from "./useJobQueue";
import {
	CancelJobResponseSchema,
	ErrorDetailSchema,
	GetJobResponseSchema,
	type Job,
	JobSchema,
	JobState,
	type JobsService,
	type WatchQueueRequest,
	type WatchQueueResponse,
	WatchQueueResponseSchema
} from "~/generated/muxmultitool/jobs/v1/jobs_pb";

function job(id: string, state: JobState, extra: Partial<Omit<Job, "$typeName" | "$unknown">> = {}): Job {
	return create(JobSchema, { id, name: id, state, command: `ffmpeg -i ${id}`, createdUnixMs: 1n, ...extra });
}

const snapshot = (eventId: number, jobs: Job[]) =>
	create(WatchQueueResponseSchema, { message: { case: "snapshot", value: { eventId: BigInt(eventId), jobs } } });
const stateEvent = (id: number, jobId: string, state: JobState) =>
	create(WatchQueueResponseSchema, {
		message: { case: "event", value: { id: BigInt(id), jobId, event: { case: "state", value: { state } } } }
	});
const logEvent = (id: number, jobId: string, line: string) =>
	create(WatchQueueResponseSchema, {
		message: { case: "event", value: { id: BigInt(id), jobId, event: { case: "logLine", value: line } } }
	});
const progressEvent = (id: number, jobId: string, percent: number) =>
	create(WatchQueueResponseSchema, {
		message: { case: "event", value: { id: BigInt(id), jobId, event: { case: "progress", value: { percent, speed: 3.4 } } } }
	});

type Connection = (signal: AbortSignal) => AsyncIterable<WatchQueueResponse>;

function connection(messages: WatchQueueResponse[], failure?: unknown): Connection {
	return async function* () {
		yield* messages;
		if (failure !== undefined) {
			throw failure;
		}
	};
}

const hanging: Connection = async function* (signal) {
	await new Promise<void>((_, reject) => signal.addEventListener("abort", () => reject(new ConnectError("aborted", Code.Canceled))));
	yield* [];
};

/** Yields `messages` and then stays open, like a healthy live stream. */
function live(messages: WatchQueueResponse[]): Connection {
	return async function* (signal) {
		yield* messages;
		yield* hanging(signal);
	};
}

const dropped = () => new ConnectError("socket closed", Code.Unavailable);

function fakeClient(connections: Connection[], overrides: Partial<Client<typeof JobsService>> = {}) {
	const resumedFrom: bigint[] = [];
	const client = {
		watchQueue: vi.fn((request: WatchQueueRequest, options?: CallOptions) => {
			resumedFrom.push(request.afterEventId);
			return (connections.shift() ?? hanging)(options?.signal ?? new AbortController().signal);
		}),
		getJob: vi.fn(async ({ jobId }: { jobId: string }) =>
			create(GetJobResponseSchema, { job: job(jobId, JobState.QUEUED, { log: ["tail"] }) })
		),
		cancelJob: vi.fn(async ({ jobId }: { jobId: string }) => create(CancelJobResponseSchema, { job: job(jobId, JobState.CANCELLED) })),
		...overrides
	} as unknown as Client<typeof JobsService>;
	return { client, resumedFrom };
}

const immediately = async () => {};

describe("useJobQueue", () => {
	it("shows the snapshot and follows the events, with the counts for the top bar", async () => {
		const { client } = fakeClient([
			live([
				snapshot(1, [job("a", JobState.RUNNING), job("b", JobState.QUEUED)]),
				progressEvent(2, "a", 50),
				logEvent(3, "a", "frame=10"),
				stateEvent(4, "a", JobState.SUCCEEDED),
				stateEvent(5, "b", JobState.RUNNING)
			])
		]);
		const queue = useJobQueue(client, { sleep: immediately });
		expect(queue.connection.value).toBe("connecting");

		await vi.waitFor(() => expect(queue.byId("a")?.state).toBe(JobState.SUCCEEDED));
		expect(queue.connection.value).toBe("live");
		expect(queue.byId("a")?.progress?.percent).toBe(50);
		expect(queue.byId("a")?.log).toEqual(["frame=10"]);
		expect(queue.list.value.map((j) => j.id)).toEqual(["b", "a"]);
		expect([queue.running.value, queue.queued.value, queue.finished.value]).toEqual([1, 0, 1]);
		expect(queue.isActive.value).toBe(true);
		queue.stop();
	});

	it("resumes from the last event id after a drop without losing or repeating events", async () => {
		const { client, resumedFrom } = fakeClient([
			connection([snapshot(1, [job("a", JobState.RUNNING)]), logEvent(2, "a", "one"), logEvent(3, "a", "two")], dropped()),
			// The server replays from id 3 on; a duplicate 3 must not show twice.
			live([logEvent(3, "a", "two"), logEvent(4, "a", "three"), stateEvent(5, "a", JobState.SUCCEEDED)])
		]);
		const queue = useJobQueue(client, { sleep: immediately });

		await vi.waitFor(() => expect(queue.byId("a")?.state).toBe(JobState.SUCCEEDED));
		expect(resumedFrom.slice(0, 2)).toEqual([0n, 3n]);
		expect(queue.byId("a")?.log).toEqual(["one", "two", "three"]);
		queue.stop();
	});

	it("keeps the log across a fresh snapshot after a gap", async () => {
		const { client } = fakeClient([
			connection(
				[snapshot(1, [job("a", JobState.RUNNING, { log: ["one"] })]), logEvent(2, "a", "two"), logEvent(3, "a", "three")],
				dropped()
			),
			live([
				snapshot(9, [
					job("a", JobState.FAILED, {
						log: ["three", "four"],
						error: create(ErrorDetailSchema, { message: "disk full", ffmpegOutput: "ENOSPC" })
					})
				])
			])
		]);
		const queue = useJobQueue(client, { sleep: immediately });

		await vi.waitFor(() => expect(queue.byId("a")?.state).toBe(JobState.FAILED));
		expect(queue.byId("a")?.log).toEqual(["one", "two", "three", "four"]);
		expect(queue.byId("a")?.error?.message).toBe("disk full");
		queue.stop();
	});

	it("fetches a job that was submitted while disconnected", async () => {
		const { client } = fakeClient([
			connection([snapshot(1, [])], dropped()),
			live([stateEvent(2, "late", JobState.QUEUED), logEvent(3, "late", "tail")])
		]);
		const queue = useJobQueue(client, { sleep: immediately });

		await vi.waitFor(() => expect(queue.byId("late")?.command).toBe("ffmpeg -i late"));
		expect(client.getJob).toHaveBeenCalledTimes(1);
		expect(queue.byId("late")?.log).toEqual(["tail"]);
		expect(queue.queued.value).toBe(1);
		queue.stop();
	});

	it("reports a lost connection with its error and recovers", async () => {
		const { client } = fakeClient([connection([snapshot(1, [])], dropped()), live([snapshot(1, [])])]);
		const seen: string[] = [];
		const queue = useJobQueue(client, {
			sleep: async () => {
				seen.push(queue.connection.value);
			}
		});

		await vi.waitFor(() => expect(seen).toEqual(["reconnecting"]));
		await vi.waitFor(() => expect(queue.connection.value).toBe("live"));
		expect(queue.connectionError.value).toBeUndefined();
		queue.stop();
	});

	it("cancels through the service and applies the answer at once", async () => {
		const { client } = fakeClient([live([snapshot(1, [job("a", JobState.QUEUED)])])]);
		const queue = useJobQueue(client, { sleep: immediately });
		await vi.waitFor(() => expect(queue.byId("a")).toBeDefined());

		await expect(queue.cancel("a")).resolves.toBeUndefined();
		expect(client.cancelJob).toHaveBeenCalledWith({ jobId: "a" });
		expect(queue.byId("a")?.state).toBe(JobState.CANCELLED);
		queue.stop();
	});

	it("returns the mapped error when a cancel is refused", async () => {
		const { client } = fakeClient([live([snapshot(1, [job("a", JobState.SUCCEEDED)])])], {
			cancelJob: vi.fn().mockRejectedValue(new ConnectError("job a has already succeeded", Code.FailedPrecondition))
		});
		const queue = useJobQueue(client, { sleep: immediately });

		await expect(queue.cancel("a")).resolves.toMatchObject({ codeName: "failed_precondition", message: "job a has already succeeded" });
		queue.stop();
	});
});
