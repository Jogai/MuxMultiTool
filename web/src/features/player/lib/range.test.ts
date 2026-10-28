import { describe, expect, it } from "vitest";

import { crossedEnd, rangeBounds, withRangeEnd, withRangeStart } from "./range";

describe("player range", () => {
	it("keeps the start before the end", () => {
		expect(withRangeStart({}, 5)).toEqual({ start: 5, end: undefined });
		expect(withRangeStart({ start: 1, end: 10 }, 5)).toEqual({ start: 5, end: 10 });
		expect(withRangeStart({ start: 1, end: 3 }, 5)).toEqual({ start: 5, end: undefined });
		expect(withRangeEnd({ start: 1 }, 10)).toEqual({ start: 1, end: 10 });
		expect(withRangeEnd({ start: 12 }, 10)).toEqual({ start: undefined, end: 10 });
	});

	it("fills unset sides with the clip bounds", () => {
		expect(rangeBounds({}, 60)).toEqual({ start: 0, end: 60 });
		expect(rangeBounds({ start: 5 }, 60)).toEqual({ start: 5, end: 60 });
		expect(rangeBounds({ end: 5 }, 60)).toEqual({ start: 0, end: 5 });
	});

	it("notices the playhead crossing the end once", () => {
		expect(crossedEnd(4.9, 5.02, 5)).toBe(true);
		expect(crossedEnd(5.02, 5.06, 5)).toBe(false);
		expect(crossedEnd(3, 4, 5)).toBe(false);
	});
});
