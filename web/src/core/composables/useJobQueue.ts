import type { Client } from "@connectrpc/connect";
import { computed, ref } from "vue";

import { type AppError, toAppError } from "~/core/libs/errors";
import { type QueueJobs, applyEvent, applyFetchedJob, applySnapshot, countJobs, queueEventId, sortJobs } from "~/core/libs/jobQueue";
import { useRpcClient } from "~/core/libs/rpc";
import { type WatchStreamOptions, watchStream } from "~/core/libs/watchStream";
import { type Job, JobsService, type WatchQueueResponse } from "~/generated/muxmultitool/jobs/v1/jobs_pb";

/** Whether the queue stream is open: the view is current only while `live`. */
export type QueueConnection = "connecting" | "live" | "reconnecting";

export interface JobQueueOptions {
	retryDelay?: WatchStreamOptions<WatchQueueResponse>["retryDelay"];
	sleep?: WatchStreamOptions<WatchQueueResponse>["sleep"];
}

/**
 * Follows the whole job queue over one resumable `WatchQueue` stream, from the moment it is created.
 * The stream carries every job's state changes, progress reports and log lines, so it feeds the queue
 * view, the job detail and the top-bar counts alike. After a drop it resumes from the last event id
 * (see `watchStream`), and a job that was submitted meanwhile is fetched with `GetJob`.
 */
export function useJobQueue(client: Client<typeof JobsService> = useRpcClient(JobsService), options: JobQueueOptions = {}) {
	const jobs = ref<QueueJobs>(new Map());
	const connection = ref<QueueConnection>("connecting");
	const connectionError = ref<AppError>();
	const fetching = new Set<string>();

	const list = computed(() => sortJobs(jobs.value));
	const counts = computed(() => countJobs(jobs.value.values()));
	const running = computed(() => counts.value.running);
	const queued = computed(() => counts.value.queued);
	const finished = computed(() => counts.value.finished);
	const isActive = computed(() => running.value > 0 || queued.value > 0);

	function byId(id: string): Job | undefined {
		return jobs.value.get(id);
	}

	async function fetchJob(id: string): Promise<void> {
		if (fetching.has(id)) {
			return;
		}
		fetching.add(id);
		try {
			const response = await client.getJob({ jobId: id });
			if (response.job) {
				applyFetchedJob(jobs.value, response.job);
			}
		} catch {
			// The next snapshot brings the job in full.
		} finally {
			fetching.delete(id);
		}
	}

	const watch = watchStream<WatchQueueResponse>({
		open: (afterEventId, signal) => client.watchQueue({ afterEventId }, { signal }),
		eventIdOf: queueEventId,
		isSnapshot: (message) => message.message.case === "snapshot",
		onMessage(message) {
			switch (message.message.case) {
				case "snapshot":
					jobs.value = applySnapshot(jobs.value, message.message.value);
					break;
				case "event": {
					const event = message.message.value;
					if (applyEvent(jobs.value, event).unknown) {
						void fetchJob(event.jobId);
					}
					break;
				}
				default:
					break;
			}
		},
		onStatus(status, error) {
			if (status !== "stopped") {
				connection.value = status;
				connectionError.value = error;
			}
		},
		retryDelay: options.retryDelay,
		sleep: options.sleep
	});

	/**
	 * Stops a running job or removes a queued one. The stream reports the state change; the response is
	 * applied right away so the row does not wait for it. Returns the error when the server refused.
	 */
	async function cancel(id: string): Promise<AppError | undefined> {
		try {
			const response = await client.cancelJob({ jobId: id });
			if (response.job) {
				applyFetchedJob(jobs.value, response.job);
			}
			return undefined;
		} catch (cause) {
			return toAppError(cause);
		}
	}

	if (typeof window !== "undefined") {
		window.addEventListener("online", watch.retryNow);
	}

	return {
		/** Every known job in view order: running, queued, then finished. */
		list,
		byId,
		running,
		queued,
		finished,
		isActive,
		connection,
		connectionError,
		cancel,
		/** Reconnects right away instead of waiting out the back-off. */
		retryNow: watch.retryNow,
		stop: watch.stop
	};
}
