/** Keeps a time inside a clip of `duration` seconds: never negative, never past the end once the length is known. */
export function clampTime(seconds: number, duration: number): number {
	if (!Number.isFinite(seconds) || seconds < 0) {
		return 0;
	}
	return duration > 0 ? Math.min(seconds, duration) : seconds;
}
