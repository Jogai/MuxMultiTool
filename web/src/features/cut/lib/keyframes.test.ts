import { describe, expect, it } from "vitest";

import { emptyKeyframeCache, isCovered, keyframeWindow, mergeKeyframes, missingRanges } from "./keyframes";

describe("keyframe cache", () => {
	it("merges fetched windows into sorted unique times and joined ranges", () => {
		let cache = mergeKeyframes(emptyKeyframeCache(), 0, 60, [0, 2, 4]);
		cache = mergeKeyframes(cache, 120, 180, [120, 124]);
		cache = mergeKeyframes(cache, 60, 120, [4, 60, 62, 120]); // the demuxer may land a little before the window
		expect(cache.times).toEqual([0, 2, 4, 60, 62, 120, 124]);
		expect(cache.covered).toEqual([[0, 180]]);
	});

	it("lists the gaps of a window", () => {
		let cache = mergeKeyframes(emptyKeyframeCache(), 0, 60, []);
		cache = mergeKeyframes(cache, 120, 180, []);
		expect(missingRanges(cache, 0, 180)).toEqual([[60, 120]]);
		expect(missingRanges(cache, 30, 200)).toEqual([
			[60, 120],
			[180, 200]
		]);
		expect(missingRanges(cache, 10, 50)).toEqual([]);
		expect(missingRanges(emptyKeyframeCache(), 10, 50)).toEqual([[10, 50]]);
		expect(isCovered(cache, 130, 170)).toBe(true);
		expect(isCovered(cache, 130, 190)).toBe(false);
	});

	it("widens and rounds the window to blocks, or takes a short file whole", () => {
		expect(keyframeWindow(10, 20, 200)).toEqual([0, 200]);
		expect(keyframeWindow(100, 130, 3600)).toEqual([60, 180]);
		expect(keyframeWindow(3500, 3600, 3600)).toEqual([3360, 3600]);
		expect(keyframeWindow(0, 30, 3600)).toEqual([0, 60]);
	});
});
