import { formatClock } from "./format";

/** Formats a span in milliseconds as `m:ss` or `h:mm:ss`, as the sketch shows ETAs (`1:12`, `6:40`). */
export function formatSpan(ms: bigint | number): string {
	return formatClock(Math.max(0, Math.round(Number(ms) / 1000)), "auto");
}

/** The speed relative to real time as `3.4×`; `undefined` while ffmpeg has not reported one. */
export function formatSpeed(speed: number | undefined): string | undefined {
	if (speed === undefined || !Number.isFinite(speed) || speed <= 0) {
		return undefined;
	}
	return `${speed < 10 ? speed.toFixed(1) : Math.round(speed).toString()}×`;
}

/** A share of 0 to 100 as `62%`, clamped. */
export function formatPercent(percent: number | undefined): string | undefined {
	if (percent === undefined || !Number.isFinite(percent)) {
		return undefined;
	}
	return `${Math.round(Math.min(100, Math.max(0, percent)))}%`;
}

/** A byte count in the nearest unit, with one decimal above kB. */
export function formatBytes(bytes: bigint | number): string {
	const units = ["B", "kB", "MB", "GB", "TB"];
	let value = Math.max(0, Number(bytes));
	let unit = 0;
	while (value >= 1000 && unit < units.length - 1) {
		value /= 1000;
		unit += 1;
	}
	return `${unit === 0 ? Math.round(value) : value.toFixed(1)} ${units[unit]}`;
}

/**
 * How long a job has been running or took: from its start to its end, or to `now` while it runs.
 * `undefined` before it started.
 */
export function elapsedMs(job: { startedUnixMs: bigint; finishedUnixMs: bigint }, now: number): number | undefined {
	if (job.startedUnixMs === 0n) {
		return undefined;
	}
	const end = job.finishedUnixMs === 0n ? now : Number(job.finishedUnixMs);
	return Math.max(0, end - Number(job.startedUnixMs));
}
