import { describe, expect, it } from "vitest";

import { clampTime, keyframeAtOrBefore, nextKeyframe, previousKeyframe, stepFrame } from "./stepping";
import { frameIndex } from "./timecode";

describe("stepFrame", () => {
	const fps = 25;
	const duration = 10;

	it("moves one frame at a time and lands on that frame", () => {
		const forward = stepFrame(1, fps, 1, duration);
		expect(frameIndex(forward, fps)).toBe(26);
		const back = stepFrame(forward, fps, -1, duration);
		expect(frameIndex(back, fps)).toBe(25);
	});

	it("does not drift when stepping back and forth repeatedly", () => {
		let time = 2;
		for (let i = 0; i < 100; i += 1) {
			time = stepFrame(time, fps, 1, duration);
		}
		expect(frameIndex(time, fps)).toBe(150);
		for (let i = 0; i < 100; i += 1) {
			time = stepFrame(time, fps, -1, duration);
		}
		expect(frameIndex(time, fps)).toBe(50);
	});

	it("stops at the first and the last frame", () => {
		expect(stepFrame(0, fps, -1, duration)).toBeCloseTo(0.001, 6);
		expect(frameIndex(stepFrame(9.99, fps, 1, duration), fps)).toBe(249);
		expect(frameIndex(stepFrame(9.99, fps, 5, duration), fps)).toBe(249);
	});

	it("works with fractional frame rates", () => {
		const t = stepFrame(0, 23.976, 1, duration);
		expect(frameIndex(t, 23.976)).toBe(1);
		expect(t).toBeCloseTo(1 / 23.976 + 0.001, 6);
	});
});

describe("keyframe stepping", () => {
	const keyframes = [0, 2.5, 5, 7.52, 10];

	it("finds the next and previous keyframe", () => {
		expect(nextKeyframe(keyframes, 3)).toBe(5);
		expect(previousKeyframe(keyframes, 3)).toBe(2.5);
	});

	it("skips the keyframe the playhead sits on", () => {
		expect(nextKeyframe(keyframes, 5)).toBe(7.52);
		expect(previousKeyframe(keyframes, 5)).toBe(2.5);
		// A seek lands a millisecond after the keyframe; that still counts as being on it
		expect(nextKeyframe(keyframes, 5.001)).toBe(7.52);
		expect(previousKeyframe(keyframes, 5.001)).toBe(2.5);
	});

	it("has no neighbour past the ends", () => {
		expect(nextKeyframe(keyframes, 10)).toBeUndefined();
		expect(previousKeyframe(keyframes, 0)).toBeUndefined();
		expect(nextKeyframe([], 1)).toBeUndefined();
	});

	it("tells where a lossless cut lands", () => {
		expect(keyframeAtOrBefore(keyframes, 6)).toBe(5);
		expect(keyframeAtOrBefore(keyframes, 5)).toBe(5);
		expect(keyframeAtOrBefore(keyframes, -1)).toBeUndefined();
	});
});

describe("clampTime", () => {
	it("keeps a seek inside the clip", () => {
		expect(clampTime(-1, 10)).toBe(0);
		expect(clampTime(11, 10)).toBe(10);
		expect(clampTime(5, 10)).toBe(5);
		expect(clampTime(Number.NaN, 10)).toBe(0);
		expect(clampTime(5, 0)).toBe(5);
	});
});
