/** How the hours of a clock reading are written. */
export type ClockHours = "auto" | "always" | "padded";

const pad = (n: number, width = 2) => String(n).padStart(width, "0");

/**
 * A whole number of seconds as a clock reading: `h:mm:ss`, or with `auto` just `m:ss` while there are
 * no hours (`1:12`, `6:40`), or with `padded` `hh:mm:ss`.
 */
export function formatClock(wholeSeconds: number, hours: ClockHours = "always"): string {
	const h = Math.floor(wholeSeconds / 3600);
	const m = Math.floor((wholeSeconds % 3600) / 60);
	const s = wholeSeconds % 60;
	if (hours === "auto" && h === 0) {
		return `${m}:${pad(s)}`;
	}
	return `${hours === "padded" ? pad(h) : h}:${pad(m)}:${pad(s)}`;
}

/** Formats a duration in seconds as `HH:MM:SS.mmm`. */
export function formatTimecode(totalSeconds: number): string {
	const ms = Math.round(totalSeconds * 1000);
	return `${formatClock(Math.floor(ms / 1000), "padded")}.${pad(ms % 1000, 3)}`;
}
