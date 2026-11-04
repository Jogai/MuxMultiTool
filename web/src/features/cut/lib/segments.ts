import { create } from "@bufbuild/protobuf";

import { lowerBound } from "~/core/libs/collections";
import { clampTime } from "~/core/libs/time";
import { CutExport, CutMode, type ProjectState, ProjectStateSchema, SegmentMode } from "~/generated/muxmultitool/cut/v1/cut_pb";

/**
 * Segment editing for the cut editor: a segment is a range of the source with a client-side id that
 * survives edits, so the table and the timeline can keep a selection while times change. Every function
 * returns a new list; the project composable commits it to the server.
 */
export interface Segment {
	id: number;
	start: number;
	end: number;
}

/** What the editor keeps for a file: the proto's `ProjectState` with ids on the segments. */
export interface EditorState {
	segments: Segment[];
	segmentMode: SegmentMode;
	cutMode: CutMode;
	export: CutExport;
}

/** The shortest a segment may be when nothing better (a frame) is known. */
export const DEFAULT_MIN_LENGTH = 0.04;

/** Tolerance for comparing times: a thousandth of a second, below what a timecode shows. */
const EPSILON = 0.0005;

/** The validation problems typed times can have; the keys of `cut.segments.errors`. */
export type TimeError = "invalid" | "negative" | "pastEnd" | "startAfterEnd";

export function emptyState(): EditorState {
	return { segments: [], segmentMode: SegmentMode.KEEP, cutMode: CutMode.KEYFRAME, export: CutExport.SEPARATE_FILES };
}

/** The editor state for a saved project; ids are assigned in order. */
export function fromProjectState(state: ProjectState | undefined): EditorState {
	if (!state) {
		return emptyState();
	}
	let id = 0;
	return {
		segments: sortSegments(state.segments.map((segment) => ({ id: ++id, start: segment.startSeconds, end: segment.endSeconds }))),
		segmentMode: state.segmentMode === SegmentMode.REMOVE ? SegmentMode.REMOVE : SegmentMode.KEEP,
		cutMode: state.cutMode === CutMode.UNSPECIFIED ? CutMode.KEYFRAME : state.cutMode,
		export: state.export === CutExport.MERGED ? CutExport.MERGED : CutExport.SEPARATE_FILES
	};
}

/** The proto state of the editor's, as `UpdateProject` takes it. */
export function toProjectState(state: EditorState): ProjectState {
	return create(ProjectStateSchema, {
		segments: state.segments.map((segment) => ({ startSeconds: segment.start, endSeconds: segment.end })),
		segmentMode: state.segmentMode,
		cutMode: state.cutMode,
		export: state.export
	});
}

/** Whether two states would save the same thing (ids do not count). */
export function statesEqual(a: EditorState, b: EditorState): boolean {
	return (
		a.segmentMode === b.segmentMode &&
		a.cutMode === b.cutMode &&
		a.export === b.export &&
		a.segments.length === b.segments.length &&
		a.segments.every((segment, i) => near(segment.start, b.segments[i]!.start) && near(segment.end, b.segments[i]!.end))
	);
}

/**
 * The editor state with the ids of `previous` kept where a segment still has the same times, so a
 * selection survives a round trip through the server (undo, redo, a reload).
 */
export function keepIds(next: EditorState, previous: EditorState): EditorState {
	let maxId = Math.max(0, ...previous.segments.map((segment) => segment.id));
	const unused = new Set(previous.segments);
	const segments = next.segments.map((segment) => {
		const match = [...unused].find((old) => near(old.start, segment.start) && near(old.end, segment.end));
		if (match) {
			unused.delete(match);
			return { ...segment, id: match.id };
		}
		return { ...segment, id: ++maxId };
	});
	return { ...next, segments };
}

export function sortSegments(segments: readonly Segment[]): Segment[] {
	return [...segments].sort((a, b) => a.start - b.start || a.end - b.end || a.id - b.id);
}

function nextId(segments: readonly Segment[]): number {
	return segments.reduce((max, segment) => Math.max(max, segment.id), 0) + 1;
}

/**
 * Adds a segment; its times are clamped to the file and ordered. Returns the list and the new id, or
 * `undefined` when nothing of the range is left inside the file (`validateTimes` refuses such times too).
 */
export function addSegment(
	segments: readonly Segment[],
	start: number,
	end: number,
	duration: number
): { segments: Segment[]; id: number } | undefined {
	const id = nextId(segments);
	const [from, to] = start <= end ? [start, end] : [end, start];
	const segment = { id, start: clampTime(from, duration), end: clampTime(to, duration) };
	if (segment.end <= segment.start) {
		return undefined;
	}
	return { segments: sortSegments([...segments, segment]), id };
}

export function removeSegment(segments: readonly Segment[], id: number): Segment[] {
	return segments.filter((segment) => segment.id !== id);
}

/** The segment moved so it starts at `start`, keeping its length and staying inside the file. */
export function moveSegment(segments: readonly Segment[], id: number, start: number, duration: number): Segment[] {
	return sortSegments(
		segments.map((segment) => {
			if (segment.id !== id) {
				return segment;
			}
			const length = segment.end - segment.start;
			const latest = duration > 0 ? Math.max(0, duration - length) : Number.POSITIVE_INFINITY;
			const from = Math.min(clampTime(start, duration), latest);
			return { ...segment, start: from, end: clampTime(from + length, duration) };
		})
	);
}

/**
 * One edge of the segment moved to `seconds`, no closer than `minLength` to the other edge and inside
 * the file; the file's bounds win over the minimum length.
 */
export function resizeSegment(
	segments: readonly Segment[],
	id: number,
	edge: "start" | "end",
	seconds: number,
	duration: number,
	minLength = DEFAULT_MIN_LENGTH
): Segment[] {
	return sortSegments(
		segments.map((segment) => {
			if (segment.id !== id) {
				return segment;
			}
			if (edge === "start") {
				return { ...segment, start: Math.max(0, Math.min(clampTime(seconds, duration), segment.end - minLength)) };
			}
			return { ...segment, end: clampTime(Math.max(clampTime(seconds, duration), segment.start + minLength), duration) };
		})
	);
}

/** The segment with both times replaced; the caller validates them first (`validateTimes`). */
export function setSegmentTimes(segments: readonly Segment[], id: number, start: number, end: number): Segment[] {
	return sortSegments(segments.map((segment) => (segment.id === id ? { ...segment, start, end } : segment)));
}

/** Why a typed start/end pair cannot be used, or `undefined` when it can. */
export function validateTimes(start: number, end: number, duration: number): TimeError | undefined {
	if (!Number.isFinite(start) || !Number.isFinite(end)) {
		return "invalid";
	}
	if (start < 0 || end < 0) {
		return "negative";
	}
	if (duration > 0 && (start > duration + EPSILON || end > duration + EPSILON)) {
		return "pastEnd";
	}
	if (end <= start) {
		return "startAfterEnd";
	}
	return undefined;
}

/**
 * The nearest keyframe within `threshold` seconds of `seconds`, or `seconds` itself when none is that
 * close. `keyframes` is ascending.
 */
export function snapToKeyframe(seconds: number, keyframes: readonly number[], threshold: number): number {
	if (keyframes.length === 0 || !(threshold > 0)) {
		return seconds;
	}
	const low = lowerBound(keyframes, seconds);
	let best: number | undefined;
	for (const candidate of [keyframes[low - 1], keyframes[low]]) {
		if (candidate !== undefined && Math.abs(candidate - seconds) <= threshold) {
			if (best === undefined || Math.abs(candidate - seconds) < Math.abs(best - seconds)) {
				best = candidate;
			}
		}
	}
	return best ?? seconds;
}

/** The segment under `seconds`, if any (the first by start when they overlap). */
export function segmentAt(segments: readonly Segment[], seconds: number): Segment | undefined {
	return segments.find((segment) => segment.start <= seconds && seconds < segment.end);
}

/** The total length of the segments, overlaps counted once. */
export function coveredSeconds(segments: readonly Segment[]): number {
	let total = 0;
	let cursor = -1;
	for (const segment of sortSegments(segments)) {
		const from = Math.max(segment.start, cursor);
		if (segment.end > from) {
			total += segment.end - from;
			cursor = segment.end;
		}
	}
	return total;
}

/** The gaps between the segments: what a REMOVE-mode export keeps. */
export function invertSegments(segments: readonly Segment[], duration: number): Array<{ start: number; end: number }> {
	const gaps: Array<{ start: number; end: number }> = [];
	let cursor = 0;
	for (const segment of sortSegments(segments)) {
		if (segment.start > cursor + EPSILON) {
			gaps.push({ start: cursor, end: Math.min(segment.start, duration) });
		}
		cursor = Math.max(cursor, segment.end);
	}
	if (duration > cursor + EPSILON) {
		gaps.push({ start: cursor, end: duration });
	}
	return gaps;
}

function near(a: number, b: number): boolean {
	return Math.abs(a - b) < EPSILON;
}
