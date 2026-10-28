import { type AppError, toAppError } from "~/core/libs/errors";

/**
 * Where a watch stream stands: `connecting` before the first message ever, `live` while messages flow,
 * `reconnecting` after a drop (with the error that caused it) and `stopped` once `stop()` was called.
 */
export type WatchStatus = "connecting" | "live" | "reconnecting" | "stopped";

export interface WatchStreamOptions<M> {
	/**
	 * Opens the server stream, asking for the events after `afterEventId` (`0n` for a fresh watch that
	 * starts with a snapshot). The signal aborts the request when the watch stops.
	 */
	open: (afterEventId: bigint, signal: AbortSignal) => AsyncIterable<M>;
	/** The event id a message carries: a snapshot's `eventId` or an event's `id`. */
	eventIdOf: (message: M) => bigint;
	/** True for a snapshot. A snapshot replaces the client's state, so it is applied even when its id is not new. */
	isSnapshot: (message: M) => boolean;
	/** Receives every message that is new to this watch, in order. */
	onMessage: (message: M) => void;
	onStatus?: (status: WatchStatus, error?: AppError) => void;
	/**
	 * Whether a stream that ends without an error is reopened. The queue stream only ends when the server
	 * shuts down, so the default is to reconnect; a single-job stream ends when the job is done.
	 */
	reconnectOnEnd?: boolean;
	/** Milliseconds to wait before reconnect attempt `attempt` (1-based). */
	retryDelay?: (attempt: number) => number;
	/** The timer, injectable for tests. Resolves early when the signal aborts. */
	sleep?: (ms: number, signal: AbortSignal) => Promise<void>;
}

export interface WatchHandle {
	/** Ends the watch: aborts the open stream and reconnects no more. */
	stop: () => void;
	/** Skips the current back-off wait, e.g. when the browser reports it is online again. */
	retryNow: () => void;
	/** The id of the last message applied; what the next reconnect resumes from. */
	readonly lastEventId: bigint;
}

/** 1 s, 2 s, 4 s, … capped at 30 s. */
export function defaultRetryDelay(attempt: number): number {
	return Math.min(1000 * 2 ** Math.max(attempt - 1, 0), 30_000);
}

function defaultSleep(ms: number, signal: AbortSignal): Promise<void> {
	return new Promise((resolve) => {
		const done = () => {
			clearTimeout(timer);
			signal.removeEventListener("abort", done);
			resolve();
		};
		const timer = setTimeout(done, ms);
		if (signal.aborted) {
			done();
		} else {
			signal.addEventListener("abort", done, { once: true });
		}
	});
}

/**
 * Keeps a resumable watch stream open, following the convention in the architecture doc: the server sends
 * a snapshot first and events after it; after a drop the client resubscribes with the last event id it
 * saw and the server replays what was missed, or sends a fresh snapshot when it no longer can.
 *
 * The runner remembers that id, reconnects with exponential back-off after a drop or an end, and drops
 * any event whose id is not above the last one applied, so a replay never duplicates an event. It never
 * throws: errors go to `onStatus` as `reconnecting`.
 */
export function watchStream<M>(options: WatchStreamOptions<M>): WatchHandle {
	const {
		open,
		eventIdOf,
		isSnapshot,
		onMessage,
		onStatus,
		reconnectOnEnd = true,
		retryDelay = defaultRetryDelay,
		sleep = defaultSleep
	} = options;
	const controller = new AbortController();
	let wake: AbortController | undefined;
	let after = 0n;
	let attempt = 0;

	async function run(): Promise<void> {
		onStatus?.("connecting");
		while (!controller.signal.aborted) {
			let failure: unknown;
			let fresh = true;
			try {
				for await (const message of open(after, controller.signal)) {
					if (controller.signal.aborted) {
						break;
					}
					const id = eventIdOf(message);
					if (!isSnapshot(message) && id <= after) {
						continue;
					}
					if (id > after) {
						after = id;
					}
					if (fresh) {
						fresh = false;
						attempt = 0;
						onStatus?.("live");
					}
					onMessage(message);
				}
			} catch (cause) {
				failure = cause;
			}
			if (controller.signal.aborted || (failure === undefined && !reconnectOnEnd)) {
				break;
			}
			attempt += 1;
			onStatus?.("reconnecting", failure === undefined ? undefined : toAppError(failure));
			wake = new AbortController();
			await sleep(retryDelay(attempt), AbortSignal.any([controller.signal, wake.signal]));
			wake = undefined;
		}
		onStatus?.("stopped");
	}

	void run();

	return {
		stop: () => controller.abort(),
		retryNow: () => wake?.abort(),
		get lastEventId() {
			return after;
		}
	};
}
