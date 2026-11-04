import { describe, expect, it } from "vitest";

import {
	addTrack,
	combineOutputName,
	editTrack,
	emptyCombineForm,
	isAdded,
	isOffsetValid,
	originOf,
	parseOffset,
	removeTrack,
	sourceLabel,
	syncSources,
	toCombineOperation
} from "./combine";

describe("combine", () => {
	it("labels sources like the backend", () => {
		expect([0, 1, 25, 26, 27].map(sourceLabel)).toEqual(["A", "B", "Z", "AA", "AB"]);
		expect(originOf({ source: 1, streamIndex: 3 })).toBe("B:#3");
	});

	it("reads offsets", () => {
		expect(parseOffset("")).toBe(0);
		expect(parseOffset(" -1.5 ")).toBe(-1.5);
		expect(parseOffset("x")).toBe(0);
		expect(isOffsetValid("")).toBe(true);
		expect(isOffsetValid("+2.000")).toBe(true);
		expect(isOffsetValid("2s")).toBe(false);
	});

	it("reads exactly the offsets it accepts", () => {
		for (const [text, seconds] of [
			[".5", 0.5],
			["5.", 5],
			["-.25", -0.25],
			["+2.000", 2]
		] as const) {
			expect(isOffsetValid(text)).toBe(true);
			expect(parseOffset(text)).toBe(seconds);
		}
		for (const text of ["2s", "1e3", "0x10", "1.2.3", "Infinity", "."]) {
			expect(isOffsetValid(text)).toBe(false);
			expect(parseOffset(text)).toBe(0);
		}
	});

	it("adds a stream once and edits tracks in place", () => {
		let tracks = addTrack([], 0, 0);
		tracks = addTrack(tracks, 1, 2);
		tracks = addTrack(tracks, 1, 2);
		expect(tracks).toEqual([
			{ source: 0, streamIndex: 0 },
			{ source: 1, streamIndex: 2 }
		]);
		expect(isAdded(tracks, 1, 2)).toBe(true);
		tracks = editTrack(tracks, 1, { language: "eng", default: true });
		expect(tracks[1]).toEqual({ source: 1, streamIndex: 2, language: "eng", default: true });
		expect(removeTrack(tracks, 0)).toEqual([tracks[1]]);
	});

	it("follows the selection and renumbers the tracks", () => {
		const form = {
			sources: [
				{ id: "a", name: "a.mkv", offset: "" },
				{ id: "b", name: "b.m2ts", offset: "2" }
			],
			tracks: [
				{ source: 0, streamIndex: 0 },
				{ source: 1, streamIndex: 1 }
			]
		};
		const synced = syncSources(form, [
			{ id: "b", name: "b.m2ts" },
			{ id: "c", name: "c.srt" }
		]);
		expect(synced.sources).toEqual([
			{ id: "b", name: "b.m2ts", offset: "2" },
			{ id: "c", name: "c.srt", offset: "" }
		]);
		expect(synced.tracks).toEqual([{ source: 0, streamIndex: 1 }]);
	});

	it("builds the operation with the output next to source A", () => {
		const form = {
			...emptyCombineForm(),
			sources: [
				{ id: "a", name: "Movie.2024.BDRip.mkv", offset: "" },
				{ id: "b", name: "Movie.m2ts", offset: "0.5" }
			],
			tracks: [
				{ source: 0, streamIndex: 0 },
				{ source: 1, streamIndex: 1, language: "eng", forced: false }
			]
		};
		expect(combineOutputName(form)).toBe("Movie.2024.BDRip.combined.mkv");
		const operation = toCombineOperation(form);
		expect(operation.output).toBe("Movie.2024.BDRip.combined.mkv");
		expect(operation.sources.map((source) => [source.input, source.offsetSeconds])).toEqual([
			["a", 0],
			["b", 0.5]
		]);
		expect(operation.tracks[1]).toMatchObject({ source: 1, streamIndex: 1, language: "eng", forced: false });
		expect(operation.tracks[1]?.title).toBeUndefined();
		expect(operation.tracks[0]?.language).toBeUndefined();
	});
});
