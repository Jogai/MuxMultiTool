import { FRAME_EPSILON, frameIndex, seekTargetForFrame } from "./timecode";

/** Keeps a seek target inside the clip. */
export function clampTime(seconds: number, duration: number): number {
	if (!Number.isFinite(seconds) || seconds < 0) {
		return 0;
	}
	return duration > 0 ? Math.min(seconds, duration) : seconds;
}

/**
 * The seek target `delta` frames away from `seconds` (negative for backwards), clamped to the first and
 * last frame of a clip of `duration` seconds.
 */
export function stepFrame(seconds: number, frameRate: number, delta: number, duration: number): number {
	const lastFrame = duration > 0 ? Math.max(0, Math.ceil(duration * frameRate - FRAME_EPSILON) - 1) : Number.POSITIVE_INFINITY;
	const frame = Math.min(Math.max(frameIndex(seconds, frameRate) + delta, 0), lastFrame);
	return clampTime(seekTargetForFrame(frame, frameRate), duration);
}

/** Keyframe times a hair apart from the playhead count as the playhead's own keyframe, not a neighbour. */
const KEYFRAME_EPSILON = 0.002;

/** The first keyframe after `seconds`, or `undefined` past the last one. `keyframes` is ascending. */
export function nextKeyframe(keyframes: readonly number[], seconds: number): number | undefined {
	const index = upperBound(keyframes, seconds + KEYFRAME_EPSILON);
	return keyframes[index];
}

/** The last keyframe before `seconds`, or `undefined` before the first one. `keyframes` is ascending. */
export function previousKeyframe(keyframes: readonly number[], seconds: number): number | undefined {
	const index = lowerBound(keyframes, seconds - KEYFRAME_EPSILON) - 1;
	return keyframes[index];
}

/** The keyframe at or before `seconds`: where a lossless cut at that time would land. */
export function keyframeAtOrBefore(keyframes: readonly number[], seconds: number): number | undefined {
	const index = upperBound(keyframes, seconds + KEYFRAME_EPSILON) - 1;
	return keyframes[index];
}

/** Index of the first element >= value. */
function lowerBound(sorted: readonly number[], value: number): number {
	let low = 0;
	let high = sorted.length;
	while (low < high) {
		const mid = (low + high) >>> 1;
		if (sorted[mid]! < value) {
			low = mid + 1;
		} else {
			high = mid;
		}
	}
	return low;
}

/** Index of the first element > value. */
function upperBound(sorted: readonly number[], value: number): number {
	let low = 0;
	let high = sorted.length;
	while (low < high) {
		const mid = (low + high) >>> 1;
		if (sorted[mid]! <= value) {
			low = mid + 1;
		} else {
			high = mid;
		}
	}
	return low;
}
