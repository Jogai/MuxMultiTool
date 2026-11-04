import { describe, expect, it } from "vitest";

import { lowerBound, move, toggled, upperBound } from "./collections";

describe("move", () => {
	it("moves an item and ignores impossible moves", () => {
		expect(move(["a", "b", "c"], 2, 0)).toEqual(["c", "a", "b"]);
		expect(move(["a", "b", "c"], 0, 1)).toEqual(["b", "a", "c"]);
		expect(move([1, 2, 3], 0, 2)).toEqual([2, 3, 1]);
		expect(move(["a", "b"], 0, 5)).toEqual(["a", "b"]);
		expect(move(["a", "b"], -1, 0)).toEqual(["a", "b"]);
		expect(move([1, 2, 3], 2, 5)).toEqual([1, 2, 3]);
	});
});

describe("toggled", () => {
	it("adds an absent item and removes a present one, leaving the input alone", () => {
		const set = new Set(["a"]);
		expect([...toggled(set, "b")]).toEqual(["a", "b"]);
		expect([...toggled(set, "a")]).toEqual([]);
		expect([...set]).toEqual(["a"]);
	});
});

describe("lowerBound and upperBound", () => {
	const sorted = [1, 3, 3, 5];

	it("find the first index at or past a value", () => {
		expect(lowerBound(sorted, 3)).toBe(1);
		expect(upperBound(sorted, 3)).toBe(3);
		expect(lowerBound(sorted, 0)).toBe(0);
		expect(upperBound(sorted, 9)).toBe(4);
		expect(lowerBound([], 1)).toBe(0);
	});

	it("take a key for objects", () => {
		const cues = [{ start: 0 }, { start: 2 }, { start: 4 }];
		expect(upperBound(cues, 2, (cue) => cue.start)).toBe(2);
		expect(lowerBound(cues, 2, (cue) => cue.start)).toBe(1);
	});
});
