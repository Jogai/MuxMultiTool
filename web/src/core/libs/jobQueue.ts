import { create } from "@bufbuild/protobuf";

import {
	type Job,
	type JobEvent,
	JobSchema,
	JobState,
	type QueueSnapshot,
	type WatchQueueResponse
} from "~/generated/muxmultitool/jobs/v1/jobs_pb";

/**
 * How many log lines the client keeps per job. The server's snapshot carries the last 200; every line
 * after that arrives as an event, so a long-running job collects its whole log here while the page is
 * open, up to this limit.
 */
export const JOB_LOG_LIMIT = 2000;

/** The jobs the client knows, by id. */
export type QueueJobs = Map<string, Job>;

/** Whether a job is in a state it never leaves. */
export function isTerminal(state: JobState): boolean {
	return state === JobState.SUCCEEDED || state === JobState.FAILED || state === JobState.CANCELLED;
}

/** The event id a watch message carries: the snapshot's `eventId` or the event's `id`. */
export function queueEventId(message: WatchQueueResponse): bigint {
	switch (message.message.case) {
		case "snapshot":
			return message.message.value.eventId;
		case "event":
			return message.message.value.id;
		default:
			return 0n;
	}
}

/**
 * Joins the log the client already holds with the tail a snapshot brings. The tail usually overlaps the
 * end of what the client has (same job, a moment later); the overlap is kept once and the rest appended,
 * so a resync snapshot never shortens the log and never repeats lines.
 */
export function mergeLog(existing: readonly string[], tail: readonly string[]): string[] {
	if (existing.length === 0) {
		return [...tail];
	}
	for (let overlap = Math.min(existing.length, tail.length); overlap > 0; overlap -= 1) {
		let matches = true;
		for (let i = 0; i < overlap; i += 1) {
			if (existing[existing.length - overlap + i] !== tail[i]) {
				matches = false;
				break;
			}
		}
		if (matches) {
			return trimLog([...existing, ...tail.slice(overlap)]);
		}
	}
	return trimLog([...existing, ...tail]);
}

/** Drops the oldest lines beyond `JOB_LOG_LIMIT`. */
export function trimLog(lines: string[]): string[] {
	return lines.length > JOB_LOG_LIMIT ? lines.slice(lines.length - JOB_LOG_LIMIT) : lines;
}

/**
 * Replaces the known jobs with the snapshot's, keeping each job's longer log history. Jobs missing from
 * the snapshot are gone (the server restarted), so they disappear here too.
 */
export function applySnapshot(jobs: QueueJobs, snapshot: QueueSnapshot): QueueJobs {
	const next: QueueJobs = new Map();
	for (const job of snapshot.jobs) {
		const known = jobs.get(job.id);
		next.set(job.id, known ? { ...job, log: mergeLog(known.log, job.log) } : job);
	}
	return next;
}

/**
 * Applies one event in place. Returns `unknown: true` when the event belongs to a job the client has not
 * seen, which happens when a job was submitted during a disconnect; the caller then fetches the job (its
 * command is only in the job, not in the event) while a stub holds the place.
 */
export function applyEvent(jobs: QueueJobs, event: JobEvent): { unknown: boolean } {
	let job = jobs.get(event.jobId);
	const unknown = job === undefined;
	if (!job) {
		job = create(JobSchema, { id: event.jobId });
		jobs.set(job.id, job);
	}
	switch (event.event.case) {
		case "state": {
			const change = event.event.value;
			job.state = change.state;
			if (change.state === JobState.RUNNING) {
				job.startedUnixMs = change.atUnixMs;
			} else if (isTerminal(change.state)) {
				job.finishedUnixMs = change.atUnixMs;
			}
			if (change.state === JobState.FAILED) {
				job.error = change.error;
			}
			break;
		}
		case "progress":
			job.progress = event.event.value;
			break;
		case "logLine":
			job.log.push(event.event.value);
			if (job.log.length > JOB_LOG_LIMIT) {
				job.log.splice(0, job.log.length - JOB_LOG_LIMIT);
			}
			break;
		default:
			break;
	}
	return { unknown };
}

/** How far along a state is: a job only ever moves up. */
function stateRank(state: JobState): number {
	if (isTerminal(state)) {
		return 3;
	}
	return state === JobState.RUNNING ? 2 : state === JobState.QUEUED ? 1 : 0;
}

/**
 * Fills in a job that a unary call returned (`GetJob` for a job an event named first, or the answer to a
 * cancel). The fetched job carries the command and name; its log tail is ignored when the job is known,
 * because the stream delivers every line of a known job as an event: a line in the tail is either
 * already here or still on its way, and taking it now would show it twice. The same goes for its
 * state when the stream has moved the job further meanwhile (a short job of a batch is queued, run and
 * finished before the fetch answers): the answer never sets a job back.
 */
export function applyFetchedJob(jobs: QueueJobs, fetched: Job): void {
	const known = jobs.get(fetched.id);
	if (!known) {
		jobs.set(fetched.id, fetched);
		return;
	}
	if (stateRank(fetched.state) >= stateRank(known.state)) {
		jobs.set(fetched.id, { ...fetched, log: known.log });
		return;
	}
	jobs.set(fetched.id, {
		...fetched,
		state: known.state,
		error: known.error ?? fetched.error,
		progress: known.progress ?? fetched.progress,
		startedUnixMs: known.startedUnixMs || fetched.startedUnixMs,
		finishedUnixMs: known.finishedUnixMs || fetched.finishedUnixMs,
		log: known.log
	});
}

const STATE_ORDER: Record<JobState, number> = {
	[JobState.RUNNING]: 0,
	[JobState.QUEUED]: 1,
	[JobState.UNSPECIFIED]: 2,
	[JobState.SUCCEEDED]: 3,
	[JobState.FAILED]: 3,
	[JobState.CANCELLED]: 3
};

/**
 * The order of the queue view, as in the sketch: running jobs first, then the queue in submission order,
 * then finished jobs with the most recent on top.
 */
export function sortJobs(jobs: QueueJobs): Job[] {
	return [...jobs.values()].sort((a, b) => {
		const group = STATE_ORDER[a.state] - STATE_ORDER[b.state];
		if (group !== 0) {
			return group;
		}
		if (isTerminal(a.state)) {
			return compare(b.finishedUnixMs, a.finishedUnixMs) || compare(b.createdUnixMs, a.createdUnixMs);
		}
		return compare(a.createdUnixMs, b.createdUnixMs) || a.id.localeCompare(b.id);
	});
}

function compare(a: bigint, b: bigint): number {
	return a < b ? -1 : a > b ? 1 : 0;
}

/** The figures for the top-bar indicator. */
export function countJobs(jobs: Iterable<Job>): { running: number; queued: number; finished: number } {
	const counts = { running: 0, queued: 0, finished: 0 };
	for (const job of jobs) {
		if (job.state === JobState.RUNNING) {
			counts.running += 1;
		} else if (job.state === JobState.QUEUED) {
			counts.queued += 1;
		} else if (isTerminal(job.state)) {
			counts.finished += 1;
		}
	}
	return counts;
}
