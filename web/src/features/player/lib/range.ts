/** A time range in the clip, for "play segment" and "loop". Either end may be unset. */
export interface PlayerRange {
	start?: number;
	end?: number;
}

/** The range with its start moved to `seconds`; an end before it is dropped. */
export function withRangeStart(range: PlayerRange, seconds: number): PlayerRange {
	return { start: seconds, end: range.end !== undefined && range.end > seconds ? range.end : undefined };
}

/** The range with its end moved to `seconds`; a start after it is dropped. */
export function withRangeEnd(range: PlayerRange, seconds: number): PlayerRange {
	return { start: range.start !== undefined && range.start < seconds ? range.start : undefined, end: seconds };
}

/** The bounds playback runs between: the clip's start and end stand in for an unset side. */
export function rangeBounds(range: PlayerRange, duration: number): { start: number; end: number } {
	return { start: range.start ?? 0, end: range.end ?? duration };
}

/** Whether the playhead crossed the range end between two ticks. */
export function crossedEnd(previous: number, now: number, end: number): boolean {
	return previous < end && now >= end;
}
