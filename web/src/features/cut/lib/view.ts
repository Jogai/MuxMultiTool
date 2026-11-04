import { formatClock } from "~/core/libs/format";
import { clampTime } from "~/core/libs/time";

/**
 * The time/pixel mapping of the timeline: a viewport of `width` pixels over a file of `duration` seconds,
 * zoomed to `pxPerSecond` and scrolled so that `scroll` seconds sit at its left edge. Every function
 * returns a new view; the component holds the current one.
 */
export interface TimelineView {
	/** Length of the file in seconds. */
	duration: number;
	/** Width of the viewport in pixels. */
	width: number;
	/** The zoom: pixels per second. */
	pxPerSecond: number;
	/** The time at the left edge of the viewport. */
	scroll: number;
}

/** How wide a single frame is at the deepest zoom, so frames can be told apart. */
export const PX_PER_FRAME_AT_MAX_ZOOM = 12;

/** The frame rate assumed for the deepest zoom when the file's is not known. */
export const FALLBACK_FRAME_RATE = 25;

/** The zoom at which the whole file fits the viewport. */
export function fitZoom(duration: number, width: number): number {
	if (!(duration > 0) || !(width > 0)) {
		return 1;
	}
	return width / duration;
}

/** The deepest zoom: single frames `PX_PER_FRAME_AT_MAX_ZOOM` pixels apart. */
export function maxZoom(frameRate: number | undefined): number {
	return (frameRate && frameRate > 0 ? frameRate : FALLBACK_FRAME_RATE) * PX_PER_FRAME_AT_MAX_ZOOM;
}

/** A view of the whole file, as the timeline starts. */
export function fitView(duration: number, width: number): TimelineView {
	return { duration, width, pxPerSecond: fitZoom(duration, width), scroll: 0 };
}

/** The pixel position of `seconds` in the viewport (may be outside 0..width). */
export function timeToX(view: TimelineView, seconds: number): number {
	return (seconds - view.scroll) * view.pxPerSecond;
}

/** The time at pixel `x` of the viewport, clamped to the file. */
export function xToTime(view: TimelineView, x: number): number {
	return clampTime(view.scroll + x / view.pxPerSecond, view.duration);
}

/** `pixels` as a span of time at the current zoom. */
export function pxToSeconds(view: TimelineView, pixels: number): number {
	return pixels / view.pxPerSecond;
}

/** How many seconds the viewport shows. */
export function visibleSeconds(view: TimelineView): number {
	return view.width / view.pxPerSecond;
}

/** The time at the right edge of the viewport. */
export function visibleEnd(view: TimelineView): number {
	return Math.min(view.duration, view.scroll + visibleSeconds(view));
}

/** The range the viewport shows. */
export function visibleRange(view: TimelineView): { from: number; to: number } {
	return { from: view.scroll, to: visibleEnd(view) };
}

/** The width of the whole file at the current zoom. */
export function contentWidth(view: TimelineView): number {
	return view.duration * view.pxPerSecond;
}

/** The view with its scroll kept inside the file: never before 0, never past the end minus one viewport. */
export function clampScroll(view: TimelineView): TimelineView {
	const maxScroll = Math.max(0, view.duration - visibleSeconds(view));
	const scroll = Math.min(Math.max(view.scroll, 0), maxScroll);
	return scroll === view.scroll ? view : { ...view, scroll };
}

/** The view scrolled so that `seconds` sits at the left edge. */
export function scrollTo(view: TimelineView, seconds: number): TimelineView {
	return clampScroll({ ...view, scroll: seconds });
}

/** The view scrolled by `pixels` (positive to the right). */
export function scrollByPx(view: TimelineView, pixels: number): TimelineView {
	return scrollTo(view, view.scroll + pxToSeconds(view, pixels));
}

/**
 * The view scrolled only as far as needed to show `seconds`: when it is off to the left it comes to the
 * left edge, when off to the right the view jumps so it sits at `fraction` of the width (the playhead then
 * has room to run on before the next jump). A visible time changes nothing.
 */
export function scrollIntoView(view: TimelineView, seconds: number, fraction = 0.1): TimelineView {
	const end = view.scroll + visibleSeconds(view);
	if (seconds >= view.scroll && seconds <= end) {
		return view;
	}
	if (seconds < view.scroll) {
		return scrollTo(view, seconds);
	}
	return scrollTo(view, seconds - visibleSeconds(view) * fraction);
}

/**
 * The view zoomed to `pxPerSecond` (between the fit and `maxZoom`) with the time under `anchorX` kept
 * in place, so zooming with the wheel stays on what the pointer is over.
 */
export function zoomTo(view: TimelineView, pxPerSecond: number, anchorX: number, frameRate?: number): TimelineView {
	const min = fitZoom(view.duration, view.width);
	const max = Math.max(min, maxZoom(frameRate));
	const next = Math.min(Math.max(pxPerSecond, min), max);
	if (next === view.pxPerSecond) {
		return view;
	}
	const anchorTime = view.scroll + anchorX / view.pxPerSecond;
	return clampScroll({ ...view, pxPerSecond: next, scroll: anchorTime - anchorX / next });
}

/** The view zoomed by `factor` (above 1 zooms in) around `anchorX`. */
export function zoomBy(view: TimelineView, factor: number, anchorX: number, frameRate?: number): TimelineView {
	return zoomTo(view, view.pxPerSecond * factor, anchorX, frameRate);
}

/** The view with another viewport width; the left edge stays where it is, within the file. */
export function resizeView(view: TimelineView, width: number, frameRate?: number): TimelineView {
	if (width === view.width || !(width > 0)) {
		return view;
	}
	const wasFit = Math.abs(view.pxPerSecond - fitZoom(view.duration, view.width)) < 1e-9;
	const resized = { ...view, width };
	// A fitted view stays fitted; any other keeps its zoom but never below the new fit
	return wasFit ? fitView(view.duration, width) : zoomTo(resized, view.pxPerSecond, 0, frameRate);
}

/** The view for another file length, zoomed to fit it. */
export function withDuration(view: TimelineView, duration: number): TimelineView {
	return fitView(duration, view.width);
}

/** Where the zoom sits between fit (0) and the deepest zoom (1), on a log scale. */
export function zoomFraction(view: TimelineView, frameRate?: number): number {
	const min = fitZoom(view.duration, view.width);
	const max = Math.max(min, maxZoom(frameRate));
	if (max <= min) {
		return 0;
	}
	return Math.min(1, Math.max(0, Math.log(view.pxPerSecond / min) / Math.log(max / min)));
}

/** The zoom at `fraction` of the log scale between fit and the deepest zoom. */
export function zoomAtFraction(view: TimelineView, fraction: number, frameRate?: number): number {
	const min = fitZoom(view.duration, view.width);
	const max = Math.max(min, maxZoom(frameRate));
	return min * Math.pow(max / min, Math.min(1, Math.max(0, fraction)));
}

/** Candidate distances between ruler ticks, in seconds, from a frame-ish step up to an hour. */
const TICK_STEPS = [0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 900, 1800, 3600];

/** How many minor ticks sit between two labelled ones, per step; two for any step not listed. */
const SUBDIVISIONS = new Map<number, number>([
	[0.01, 4],
	[0.05, 5],
	[0.1, 4],
	[0.5, 5],
	[1, 4],
	[5, 5],
	[15, 3],
	[300, 5],
	[900, 3]
]);

function subdivisions(step: number): number {
	return SUBDIVISIONS.get(step) ?? 2;
}

/**
 * The distance between labelled ticks for a zoom: the smallest step whose ticks are at least
 * `minSpacingPx` apart. A step of one frame is used when frames are wider than that.
 */
export function tickStep(pxPerSecond: number, minSpacingPx = 90, frameRate?: number): number {
	if (frameRate && frameRate > 0 && pxPerSecond / frameRate >= minSpacingPx) {
		return 1 / frameRate;
	}
	for (const step of TICK_STEPS) {
		if (step * pxPerSecond >= minSpacingPx) {
			return step;
		}
	}
	return TICK_STEPS.at(-1) ?? 3600;
}

export interface Tick {
	seconds: number;
	x: number;
	/** A labelled tick; the others are the minor marks between two labels. */
	major: boolean;
}

/** The ruler ticks of the visible range. */
export function ticks(view: TimelineView, minSpacingPx = 90, frameRate?: number): Tick[] {
	if (!(view.duration > 0) || !(view.width > 0)) {
		return [];
	}
	const step = tickStep(view.pxPerSecond, minSpacingPx, frameRate);
	// A frame step has no marks between frames
	const minor = TICK_STEPS.includes(step) ? step / subdivisions(step) : step;
	const { from, to } = visibleRange(view);
	const first = Math.floor(from / minor);
	const last = Math.ceil(to / minor);
	const result: Tick[] = [];
	for (let i = first; i <= last; i += 1) {
		const seconds = i * minor;
		if (seconds < 0 || seconds > view.duration + 1e-9) {
			continue;
		}
		const ratio = seconds / step;
		result.push({ seconds, x: timeToX(view, seconds), major: Math.abs(ratio - Math.round(ratio)) < 1e-6 });
	}
	return result;
}

/**
 * A ruler label: `H:MM:SS` for steps of a second or more (hours only when there are any), with the
 * milliseconds for finer steps.
 */
export function tickLabel(seconds: number, step: number): string {
	const whole = Math.floor(seconds + 1e-9);
	const clock = formatClock(whole, "auto");
	if (step >= 1) {
		return clock;
	}
	const ms = Math.round((seconds - whole) * 1000);
	return `${clock}.${String(ms).padStart(3, "0")}`;
}
