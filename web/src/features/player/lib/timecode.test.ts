import { describe, expect, it } from "vitest";

import { formatTimecode } from "~/core/libs/format";

import { formatFrameTimecode, frameIndex, frameStart, parseTimecode, seekTargetForFrame } from "./timecode";

describe("timecode display", () => {
	it("shows the playhead as HH:MM:SS.mmm", () => {
		expect(formatTimecode(0)).toBe("00:00:00.000");
		expect(formatTimecode(754.567)).toBe("00:12:34.567");
		expect(formatTimecode(3600 + 59 * 60 + 59.9994)).toBe("01:59:59.999");
	});

	it("maps a time to the frame on screen", () => {
		expect(frameIndex(0, 25)).toBe(0);
		expect(frameIndex(0.039, 25)).toBe(0);
		expect(frameIndex(0.04, 25)).toBe(1);
		// 24 frames at 24 fps is exactly 1 s, even when the float lands a hair before
		expect(frameIndex(0.9999999, 24)).toBe(24);
		expect(frameIndex(10, 23.976)).toBe(239);
		expect(frameIndex(-1, 25)).toBe(0);
		expect(frameIndex(5, 0)).toBe(0);
	});

	it("gives a frame's start time and a seek target just after it", () => {
		expect(frameStart(25, 25)).toBe(1);
		expect(seekTargetForFrame(25, 25)).toBeCloseTo(1.001, 6);
		// At high rates the bias stays below a quarter frame
		expect(seekTargetForFrame(0, 1000)).toBeCloseTo(0.00025, 8);
		expect(frameIndex(seekTargetForFrame(119, 23.976), 23.976)).toBe(119);
	});

	it("formats a frame-based timecode", () => {
		expect(formatFrameTimecode(0, 25)).toBe("00:00:00:00");
		expect(formatFrameTimecode(1.04, 25)).toBe("00:00:01:01");
		expect(formatFrameTimecode(3661.5, 24)).toBe("01:01:01:12");
	});
});

describe("parseTimecode", () => {
	it("accepts seconds, MM:SS and HH:MM:SS with an optional fraction", () => {
		expect(parseTimecode("12")).toBe(12);
		expect(parseTimecode("754.5")).toBe(754.5);
		expect(parseTimecode("1:02")).toBe(62);
		expect(parseTimecode("01:02:03")).toBe(3723);
		expect(parseTimecode("00:12:34.567")).toBeCloseTo(754.567, 6);
		expect(parseTimecode("0:12:34,5")).toBeCloseTo(754.5, 6);
		expect(parseTimecode(" 1:02:03.4 ")).toBeCloseTo(3723.4, 6);
	});

	it("rejects clock digits out of range and anything else", () => {
		expect(parseTimecode("1:75")).toBeUndefined();
		expect(parseTimecode("1:75:00")).toBeUndefined();
		expect(parseTimecode("abc")).toBeUndefined();
		expect(parseTimecode("-5")).toBeUndefined();
		expect(parseTimecode("")).toBeUndefined();
		expect(parseTimecode("1:2:3:4")).toBeUndefined();
	});

	it("reads the HH:MM:SS:FF form back when it knows the frame rate", () => {
		expect(parseTimecode("00:00:01:01", 25)).toBeCloseTo(1.04, 6);
		expect(parseTimecode("01:01:01:12", 24)).toBe(3661.5);
		expect(frameIndex(parseTimecode(formatFrameTimecode(10, 23.976), 23.976)!, 23.976)).toBe(239);
		expect(parseTimecode("00:00:01:25", 25)).toBeUndefined();
		expect(parseTimecode("00:01:01", 25)).toBe(61);
		expect(parseTimecode("00:00:01:01")).toBeUndefined();
		expect(parseTimecode("01:01:01", 25)).toBe(3661);
		expect(parseTimecode("1:01:01", 25)).toBe(3661);
		expect(parseTimecode("1:01:01:01", 0)).toBeUndefined();
		expect(parseTimecode("1:01:01:01", 25)).toBe(3661.04);
	});
});
