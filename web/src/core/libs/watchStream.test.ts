import { Code, ConnectError } from "@connectrpc/connect";
import { describe, expect, it, vi } from "vitest";

import { type WatchStatus, defaultRetryDelay, watchStream } from "./watchStream";

interface Msg {
	kind: "snapshot" | "event";
	id: bigint;
}

const snapshot = (id: number): Msg => ({ kind: "snapshot", id: BigInt(id) });
const event = (id: number): Msg => ({ kind: "event", id: BigInt(id) });

/** One scripted connection: the messages it yields, then how it ends. */
type Connection = (after: bigint, signal: AbortSignal) => AsyncIterable<Msg>;

/** Yields `messages`, then throws `failure` (a dropped connection) or ends cleanly. */
function connection(messages: Msg[], failure?: unknown): Connection {
	return async function* () {
		yield* messages;
		if (failure !== undefined) {
			throw failure;
		}
	};
}

/** Stays open until the watch aborts it, like a live stream with nothing to say. */
const hanging: Connection = async function* (_after, signal) {
	await new Promise<void>((_, reject) => signal.addEventListener("abort", () => reject(new ConnectError("aborted", Code.Canceled))));
	yield* [];
};

/** Hands out the scripted connections in order and records what each was asked to resume from. */
function scripted(connections: Connection[]) {
	const opened: bigint[] = [];
	const signals: AbortSignal[] = [];
	const open = (after: bigint, signal: AbortSignal) => {
		opened.push(after);
		signals.push(signal);
		return (connections.shift() ?? hanging)(after, signal);
	};
	return { open, opened, signals };
}

const dropped = () => new ConnectError("socket closed", Code.Unavailable);

function harness(connections: Connection[], overrides: { retryDelay?: (attempt: number) => number; reconnectOnEnd?: boolean } = {}) {
	const script = scripted(connections);
	const received: Msg[] = [];
	const statuses: WatchStatus[] = [];
	const errors: (string | undefined)[] = [];
	const delays: number[] = [];
	const handle = watchStream<Msg>({
		open: script.open,
		eventIdOf: (m) => m.id,
		isSnapshot: (m) => m.kind === "snapshot",
		onMessage: (m) => received.push(m),
		onStatus: (status, error) => {
			statuses.push(status);
			if (status === "reconnecting") {
				errors.push(error?.message);
			}
		},
		sleep: async (ms) => {
			delays.push(ms);
		},
		...overrides
	});
	return { ...script, received, statuses, errors, delays, handle };
}

describe("watchStream", () => {
	it("starts with a snapshot and resumes from the last event id after a drop", async () => {
		const w = harness([connection([snapshot(5), event(6), event(7)], dropped()), connection([event(8)])]);

		await vi.waitFor(() => expect(w.opened).toHaveLength(3));
		expect(w.opened).toEqual([0n, 7n, 8n]);
		expect(w.received.map((m) => Number(m.id))).toEqual([5, 6, 7, 8]);
		expect(w.handle.lastEventId).toBe(8n);
		w.handle.stop();
	});

	it("drops replayed events it already applied, so a resume never duplicates", async () => {
		const w = harness([connection([snapshot(5), event(6), event(7)], dropped()), connection([event(6), event(7), event(8)], dropped())]);

		await vi.waitFor(() => expect(w.opened).toHaveLength(3));
		expect(w.received.map((m) => Number(m.id))).toEqual([5, 6, 7, 8]);
		w.handle.stop();
	});

	it("applies a fresh snapshot after a gap the server could not replay, even at the same id", async () => {
		const w = harness([connection([snapshot(5), event(6)], dropped()), connection([snapshot(6), event(9)])]);

		await vi.waitFor(() => expect(w.received).toHaveLength(4));
		expect(w.received).toEqual([snapshot(5), event(6), snapshot(6), event(9)]);
		expect(w.handle.lastEventId).toBe(9n);
		w.handle.stop();
	});

	it("reports the connection states and the error behind a drop", async () => {
		const w = harness([connection([snapshot(1)], dropped()), connection([event(2)], dropped())]);

		await vi.waitFor(() => expect(w.statuses).toEqual(["connecting", "live", "reconnecting", "live", "reconnecting"]));
		expect(w.errors).toEqual(["socket closed", "socket closed"]);
		w.handle.stop();
		await vi.waitFor(() => expect(w.statuses.at(-1)).toBe("stopped"));
	});

	it("reconnects after a clean end by default, since the queue stream only ends with the server", async () => {
		const w = harness([connection([snapshot(1)]), connection([event(2)])]);

		await vi.waitFor(() => expect(w.opened).toEqual([0n, 1n, 2n]));
		expect(w.errors).toEqual([undefined, undefined]);
		w.handle.stop();
	});

	it("ends with the stream when asked not to reconnect, as for a finished job", async () => {
		const w = harness([connection([snapshot(1), event(2)])], { reconnectOnEnd: false });

		await vi.waitFor(() => expect(w.statuses.at(-1)).toBe("stopped"));
		expect(w.opened).toEqual([0n]);
		expect(w.received).toHaveLength(2);
	});

	it("backs off between failed attempts and starts over once messages flow again", async () => {
		const w = harness([
			connection([snapshot(1)], dropped()),
			connection([], dropped()),
			connection([], dropped()),
			connection([event(2)], dropped()),
			connection([event(3)])
		]);

		await vi.waitFor(() => expect(w.opened).toHaveLength(6));
		expect(w.delays).toEqual([1000, 2000, 4000, 1000, 1000]);
		w.handle.stop();
	});

	it("stops: aborts the open stream and reconnects no more", async () => {
		const w = harness([connection([snapshot(1)], dropped())]);
		await vi.waitFor(() => expect(w.opened).toHaveLength(2));

		w.handle.stop();

		await vi.waitFor(() => expect(w.statuses.at(-1)).toBe("stopped"));
		expect(w.signals.every((s) => s.aborted)).toBe(true);
		expect(w.opened).toHaveLength(2);
		expect(w.statuses.filter((s) => s === "reconnecting")).toHaveLength(1);
	});

	it("retries at once on request instead of waiting out the back-off", async () => {
		const script = scripted([connection([snapshot(1)], dropped()), connection([event(2)])]);
		const statuses: WatchStatus[] = [];
		const handle = watchStream<Msg>({
			open: script.open,
			eventIdOf: (m) => m.id,
			isSnapshot: (m) => m.kind === "snapshot",
			onMessage: () => {},
			onStatus: (s) => statuses.push(s),
			retryDelay: () => 60_000
		});
		await vi.waitFor(() => expect(statuses.at(-1)).toBe("reconnecting"));
		expect(script.opened).toEqual([0n]);

		handle.retryNow();

		await vi.waitFor(() => expect(script.opened).toEqual([0n, 1n]));
		handle.stop();
	});
});

describe("defaultRetryDelay", () => {
	it("doubles from one second and caps at thirty", () => {
		expect([1, 2, 3, 4, 5, 6, 10].map(defaultRetryDelay)).toEqual([1000, 2000, 4000, 8000, 16_000, 30_000, 30_000]);
	});
});
