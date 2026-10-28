/** Formats a duration in seconds as `HH:MM:SS.mmm`. */
export function formatTimecode(totalSeconds: number): string {
	const ms = Math.round(totalSeconds * 1000);
	const h = Math.floor(ms / 3_600_000);
	const m = Math.floor((ms % 3_600_000) / 60_000);
	const s = Math.floor((ms % 60_000) / 1000);
	const pad = (n: number, width = 2) => String(n).padStart(width, "0");
	return `${pad(h)}:${pad(m)}:${pad(s)}.${pad(ms % 1000, 3)}`;
}
