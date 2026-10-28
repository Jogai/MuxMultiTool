import { describe, expect, it } from "vitest";

import { formatTimecode } from "./format";

describe("formatTimecode", () => {
	it("formats hours, minutes, seconds and milliseconds", () => {
		expect(formatTimecode(3723.5)).toBe("01:02:03.500");
	});
});
