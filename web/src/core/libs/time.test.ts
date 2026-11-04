import { describe, expect, it } from "vitest";

import { clampTime } from "./time";

describe("clampTime", () => {
	it("keeps a time inside the clip", () => {
		expect(clampTime(-1, 10)).toBe(0);
		expect(clampTime(11, 10)).toBe(10);
		expect(clampTime(5, 10)).toBe(5);
		expect(clampTime(Number.NaN, 10)).toBe(0);
		expect(clampTime(5, 0)).toBe(5);
	});
});
