import { formatClock } from "~/core/libs/format";

/**
 * Timecode arithmetic for the player: frame numbers from times and back, and the typed-in forms a user
 * may enter to seek. The display string itself is `formatTimecode` in `core/libs/format.ts`.
 */

/**
 * Tolerance, in frames, when a float time sits a hair before a frame boundary (e.g. 0.9999999 s for frame
 * 24 at 24 fps). A ten-thousandth of a frame is far below anything a seek bias or a timestamp carries.
 */
export const FRAME_EPSILON = 1e-4;

/** The frame that is on screen at `seconds`: the last frame whose start is at or before that time. */
export function frameIndex(seconds: number, frameRate: number): number {
	if (!(frameRate > 0) || !Number.isFinite(seconds)) {
		return 0;
	}
	return Math.max(0, Math.floor(seconds * frameRate + FRAME_EPSILON));
}

/** The start time of frame `frame`. */
export function frameStart(frame: number, frameRate: number): number {
	return frameRate > 0 ? frame / frameRate : 0;
}

/**
 * Where to seek so that the browser presents frame `frame` and not the one before it. A seek lands on
 * the frame whose timestamp is at or before the target, so the target is nudged a little past the frame
 * start, but by less than a frame.
 */
export function seekTargetForFrame(frame: number, frameRate: number): number {
	if (!(frameRate > 0)) {
		return 0;
	}
	const bias = Math.min(0.001, 1 / (frameRate * 4));
	return frameStart(frame, frameRate) + bias;
}

/** `HH:MM:SS:FF`, the frame-based form editors show (non-drop-frame). */
export function formatFrameTimecode(seconds: number, frameRate: number): string {
	const rate = Math.round(frameRate);
	const frame = frameIndex(seconds, frameRate);
	const whole = rate > 0 ? Math.floor(frame / rate) : 0;
	const ff = rate > 0 ? frame % rate : 0;
	return `${formatClock(whole, "padded")}:${String(ff).padStart(2, "0")}`;
}

const TIMECODE = /^\s*(?:(\d+):)?(?:(\d{1,2}):)?(\d{1,2}|\d+)(?:[.,](\d{1,3})|:(\d{1,2}))?\s*$/;

/**
 * Parses a time a user typed: `SS`, `MM:SS`, `HH:MM:SS`, each with an optional `.mmm` fraction, or plain
 * seconds such as `754.5`. With a `frameRate`, the `HH:MM:SS:FF` form of `formatFrameTimecode` is read
 * back too (a frame count, so the frame number must be below the rate). Returns `undefined` for
 * anything else, including negative values.
 */
export function parseTimecode(text: string, frameRate?: number): number | undefined {
	const match = TIMECODE.exec(text);
	if (!match) {
		return undefined;
	}
	const [, first, second, last, fraction, frames] = match;
	const parts = [first, second].filter((part): part is string => part !== undefined).map(Number);
	const seconds = Number(last);
	// With a colon in front, the seconds (and the minutes after hours) are clock digits
	if (parts.length > 0 && seconds > 59) {
		return undefined;
	}
	if (parts.length === 2 && parts[1]! > 59) {
		return undefined;
	}
	let total = seconds;
	if (parts.length === 1) {
		total += parts[0]! * 60; // `MM:SS`
	} else if (parts.length === 2) {
		total += parts[0]! * 3600 + parts[1]! * 60; // `HH:MM:SS`
	}
	if (frames !== undefined) {
		// `HH:MM:SS:FF` counts frames, as `formatFrameTimecode` does: the clock part is whole frames at the rounded rate
		const rate = frameRate === undefined ? 0 : Math.round(frameRate);
		const frame = Number(frames);
		if (parts.length !== 2 || !(rate > 0) || frame >= rate) {
			return undefined;
		}
		return frameStart(total * rate + frame, frameRate!);
	}
	const ms = fraction === undefined ? 0 : Number(fraction.padEnd(3, "0"));
	return total + ms / 1000;
}
