import { create } from "@bufbuild/protobuf";
import { describe, expect, it } from "vitest";

import { CutExport, CutMode, ProjectStateSchema, SegmentMode } from "~/generated/muxmultitool/cut/v1/cut_pb";

import {
	addSegment,
	coveredSeconds,
	emptyState,
	fromProjectState,
	invertSegments,
	keepIds,
	moveSegment,
	removeSegment,
	resizeSegment,
	type Segment,
	segmentAt,
	setSegmentTimes,
	snapToKeyframe,
	statesEqual,
	toProjectState,
	validateTimes
} from "./segments";

const segments: Segment[] = [
	{ id: 1, start: 10, end: 20 },
	{ id: 2, start: 30, end: 45 }
];

describe("segment editing", () => {
	it("adds a segment in order, with the times sorted and clamped", () => {
		const added = addSegment(segments, 5, 2, 100)!;
		expect(added.id).toBe(3);
		expect(added.segments.map((segment) => segment.id)).toEqual([3, 1, 2]);
		expect(added.segments[0]).toEqual({ id: 3, start: 2, end: 5 });
		expect(addSegment(segments, 90, 500, 100)?.segments.at(-1)).toEqual({ id: 3, start: 90, end: 100 });
	});

	it("adds nothing for a range that is empty once clamped, as validateTimes would refuse it", () => {
		expect(addSegment(segments, 5, 5, 100)).toBeUndefined();
		expect(addSegment(segments, 100, 100, 100)).toBeUndefined();
		expect(addSegment(segments, 150, 200, 100)).toBeUndefined();
		expect(addSegment(segments, -5, -1, 100)).toBeUndefined();
		expect(addSegment(segments, 5, 5, 0)).toBeUndefined();
	});

	it("removes by id", () => {
		expect(removeSegment(segments, 1)).toEqual([segments[1]]);
		expect(removeSegment(segments, 9)).toEqual(segments);
	});

	it("moves a segment keeping its length inside the file", () => {
		expect(moveSegment(segments, 1, 50, 100).map((segment) => segment.id)).toEqual([2, 1]);
		expect(moveSegment(segments, 1, -5, 100)[0]).toEqual({ id: 1, start: 0, end: 10 });
		expect(moveSegment(segments, 2, 95, 100).at(-1)).toEqual({ id: 2, start: 85, end: 100 });
		// Longer than the file (a project saved for another version): it ends at the file's end
		expect(moveSegment([{ id: 1, start: 0, end: 150 }], 1, 10, 100)[0]).toEqual({ id: 1, start: 0, end: 100 });
		// An unknown duration does not pin it to the start
		expect(moveSegment(segments, 1, 50, 0).at(-1)).toEqual({ id: 1, start: 50, end: 60 });
	});

	it("resizes an edge no closer than the minimum length", () => {
		expect(resizeSegment(segments, 1, "start", 15, 100)[0]?.start).toBe(15);
		expect(resizeSegment(segments, 1, "start", 25, 100, 1)[0]?.start).toBe(19);
		expect(resizeSegment(segments, 1, "end", 5, 100, 1)[0]?.end).toBe(11);
		expect(resizeSegment(segments, 2, "end", 500, 100)[1]?.end).toBe(100);
		expect(resizeSegment(segments, 1, "start", -3, 100)[0]?.start).toBe(0);
		// The minimum length never pushes an edge out of the file
		expect(resizeSegment([{ id: 1, start: 0.01, end: 0.02 }], 1, "start", 0, 100, 1)[0]?.start).toBe(0);
		expect(resizeSegment([{ id: 1, start: 99.5, end: 100 }], 1, "end", 100, 100, 1)[0]?.end).toBe(100);
	});

	it("sets typed times and re-sorts", () => {
		const next = setSegmentTimes(segments, 1, 50, 60);
		expect(next.map((segment) => segment.id)).toEqual([2, 1]);
	});

	it("validates typed times", () => {
		expect(validateTimes(1, 2, 100)).toBeUndefined();
		expect(validateTimes(1, 2, 0)).toBeUndefined();
		expect(validateTimes(Number.NaN, 2, 100)).toBe("invalid");
		expect(validateTimes(-1, 2, 100)).toBe("negative");
		expect(validateTimes(1, 101, 100)).toBe("pastEnd");
		expect(validateTimes(2, 2, 100)).toBe("startAfterEnd");
		expect(validateTimes(3, 2, 100)).toBe("startAfterEnd");
	});

	it("snaps to the nearest keyframe within the threshold", () => {
		const keyframes = [0, 2, 4.5, 10];
		expect(snapToKeyframe(2.2, keyframes, 0.5)).toBe(2);
		expect(snapToKeyframe(4.3, keyframes, 0.5)).toBe(4.5);
		expect(snapToKeyframe(3.3, keyframes, 1.3)).toBe(4.5);
		expect(snapToKeyframe(7, keyframes, 0.5)).toBe(7);
		expect(snapToKeyframe(7, [], 0.5)).toBe(7);
		expect(snapToKeyframe(10.1, keyframes, 0.5)).toBe(10);
		expect(snapToKeyframe(-0.1, keyframes, 0.5)).toBe(0);
	});

	it("finds the segment under a time", () => {
		expect(segmentAt(segments, 15)?.id).toBe(1);
		expect(segmentAt(segments, 20)).toBeUndefined();
		expect(segmentAt(segments, 30)?.id).toBe(2);
	});

	it("sums the covered time once and inverts to the gaps", () => {
		expect(coveredSeconds(segments)).toBe(25);
		expect(coveredSeconds([...segments, { id: 3, start: 15, end: 35 }])).toBe(35);
		expect(invertSegments(segments, 100)).toEqual([
			{ start: 0, end: 10 },
			{ start: 20, end: 30 },
			{ start: 45, end: 100 }
		]);
		expect(invertSegments([{ id: 1, start: 0, end: 100 }], 100)).toEqual([]);
	});
});

describe("project state", () => {
	it("round-trips through the proto with ids assigned", () => {
		const proto = create(ProjectStateSchema, {
			segments: [
				{ startSeconds: 30, endSeconds: 45 },
				{ startSeconds: 10, endSeconds: 20 }
			],
			segmentMode: SegmentMode.REMOVE,
			cutMode: CutMode.SMART,
			export: CutExport.MERGED
		});
		const state = fromProjectState(proto);
		expect(state.segments).toEqual([
			{ id: 2, start: 10, end: 20 },
			{ id: 1, start: 30, end: 45 }
		]);
		expect(state.segmentMode).toBe(SegmentMode.REMOVE);
		expect(state.cutMode).toBe(CutMode.SMART);
		expect(state.export).toBe(CutExport.MERGED);
		expect(toProjectState(state).segments).toMatchObject([
			{ startSeconds: 10, endSeconds: 20 },
			{ startSeconds: 30, endSeconds: 45 }
		]);
	});

	it("reads unspecified modes as the defaults", () => {
		const state = fromProjectState(create(ProjectStateSchema, {}));
		expect(state).toEqual(emptyState());
		expect(fromProjectState(undefined)).toEqual(emptyState());
	});

	it("compares states by times and modes, not ids", () => {
		const a = { ...emptyState(), segments };
		const b = { ...emptyState(), segments: segments.map((segment) => ({ ...segment, id: segment.id + 10 })) };
		expect(statesEqual(a, b)).toBe(true);
		expect(statesEqual(a, { ...b, cutMode: CutMode.NORMAL })).toBe(false);
		expect(statesEqual(a, { ...b, segments: [segments[0]!] })).toBe(false);
	});

	it("keeps the ids of unchanged segments across a server round trip", () => {
		const previous = { ...emptyState(), segments };
		const fromServer = {
			...emptyState(),
			segments: [
				{ id: 1, start: 10, end: 20 },
				{ id: 2, start: 30, end: 50 },
				{ id: 3, start: 60, end: 70 }
			]
		};
		expect(keepIds(fromServer, previous).segments.map((segment) => segment.id)).toEqual([1, 3, 4]);
	});
});
