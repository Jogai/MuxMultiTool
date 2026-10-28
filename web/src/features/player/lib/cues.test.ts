import { create } from "@bufbuild/protobuf";
import { describe, expect, it } from "vitest";

import { StreamSummarySchema, StreamType } from "~/generated/muxmultitool/files/v1/files_pb";
import { SubtitleCueSchema } from "~/generated/muxmultitool/media/v1/media_pb";

import { activeCues, isTextSubtitle, sortCues, subtitleStreams } from "./cues";

const cue = (startSeconds: number, endSeconds: number, text: string) => create(SubtitleCueSchema, { startSeconds, endSeconds, text });

describe("activeCues", () => {
	const cues = sortCues([cue(5, 8, "c"), cue(0, 2, "a"), cue(1.5, 3, "b"), cue(7, 20, "d"), cue(30, 31, "e")]);

	it("returns the cues that cover the time, in start order", () => {
		expect(activeCues(cues, 0).map((c) => c.text)).toEqual(["a"]);
		expect(activeCues(cues, 1.7).map((c) => c.text)).toEqual(["a", "b"]);
		expect(activeCues(cues, 2).map((c) => c.text)).toEqual(["b"]);
		expect(activeCues(cues, 7.5).map((c) => c.text)).toEqual(["c", "d"]);
		expect(activeCues(cues, 15).map((c) => c.text)).toEqual(["d"]);
	});

	it("returns nothing between cues and outside them", () => {
		expect(activeCues(cues, 4)).toEqual([]);
		expect(activeCues(cues, 25)).toEqual([]);
		expect(activeCues(cues, -1)).toEqual([]);
		expect(activeCues([], 1)).toEqual([]);
	});
});

describe("subtitle streams", () => {
	it("picks the subtitle streams and tells text from image codecs", () => {
		const streams = [
			create(StreamSummarySchema, { index: 0, type: StreamType.VIDEO, codec: "h264" }),
			create(StreamSummarySchema, { index: 2, type: StreamType.SUBTITLE, codec: "subrip", language: "eng" }),
			create(StreamSummarySchema, { index: 3, type: StreamType.SUBTITLE, codec: "hdmv_pgs_subtitle", language: "ger" })
		];
		expect(subtitleStreams(streams).map((s) => s.index)).toEqual([2, 3]);
		expect(isTextSubtitle("subrip")).toBe(true);
		expect(isTextSubtitle("ass")).toBe(true);
		expect(isTextSubtitle("hdmv_pgs_subtitle")).toBe(false);
		expect(isTextSubtitle("dvd_subtitle")).toBe(false);
	});
});
