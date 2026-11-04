/**
 * The keyframe cache of a file. `GetKeyframes` reads the file between two times, so the editor asks for
 * windows around what the timeline shows and remembers which ranges it has; the merged, sorted list feeds
 * the keyframe lane, snapping and the player's keyframe stepping.
 */
export interface KeyframeCache {
	/** Every keyframe time known so far, ascending and unique. */
	times: readonly number[];
	/** The ranges that have been fetched, sorted and non-overlapping. */
	covered: ReadonlyArray<readonly [number, number]>;
}

/** Windows are rounded out to multiples of this, so small view changes reuse the same fetches. */
export const KEYFRAME_BLOCK_SECONDS = 60;

export function emptyKeyframeCache(): KeyframeCache {
	return { times: [], covered: [] };
}

/** The cache with the keyframes of `[from, to]` added. */
export function mergeKeyframes(cache: KeyframeCache, from: number, to: number, times: readonly number[]): KeyframeCache {
	const merged = [...cache.times, ...times].sort((a, b) => a - b);
	const unique: number[] = [];
	for (const time of merged) {
		const last = unique.at(-1);
		if (last === undefined || time - last > 1e-6) {
			unique.push(time);
		}
	}
	const covered = [...cache.covered, [from, to] as const].sort((a, b) => a[0] - b[0]);
	const joined: Array<[number, number]> = [];
	for (const range of covered) {
		const last = joined.at(-1);
		if (last && range[0] <= last[1] + 1e-9) {
			last[1] = Math.max(last[1], range[1]);
		} else {
			joined.push([range[0], range[1]]);
		}
	}
	return { times: unique, covered: joined };
}

/** The parts of `[from, to]` the cache does not cover yet, in order. */
export function missingRanges(cache: KeyframeCache, from: number, to: number): Array<[number, number]> {
	const gaps: Array<[number, number]> = [];
	let cursor = from;
	for (const [start, end] of cache.covered) {
		if (end <= cursor) {
			continue;
		}
		if (start >= to) {
			break;
		}
		if (start > cursor) {
			gaps.push([cursor, Math.min(start, to)]);
		}
		cursor = Math.max(cursor, end);
		if (cursor >= to) {
			break;
		}
	}
	if (cursor < to) {
		gaps.push([cursor, to]);
	}
	return gaps;
}

/**
 * The window to fetch for a visible range: widened by one viewport on each side so scrolling has
 * keyframes ready, then rounded out to whole blocks and clamped to the file. A file shorter than a few
 * blocks is fetched whole.
 */
export function keyframeWindow(from: number, to: number, duration: number, block = KEYFRAME_BLOCK_SECONDS): [number, number] {
	if (duration <= block * 4) {
		return [0, duration];
	}
	const span = Math.max(to - from, 0);
	const start = Math.max(0, Math.floor((from - span) / block) * block);
	const end = Math.min(duration, Math.ceil((to + span) / block) * block);
	return [start, end];
}

/** Whether the cache covers the whole window. */
export function isCovered(cache: KeyframeCache, from: number, to: number): boolean {
	return missingRanges(cache, from, to).length === 0;
}
