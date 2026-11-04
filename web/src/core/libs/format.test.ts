import { describe, expect, it } from "vitest";

import { formatClock, formatTimecode } from "./format";

describe("formatClock", () => {
	it("writes the hours as asked", () => {
		expect(formatClock(141)).toBe("0:02:21");
		expect(formatClock(141, "auto")).toBe("2:21");
		expect(formatClock(141, "padded")).toBe("00:02:21");
		expect(formatClock(7293, "auto")).toBe("2:01:33");
		expect(formatClock(0, "auto")).toBe("0:00");
	});
});

describe("formatTimecode", () => {
	it("formats hours, minutes, seconds and milliseconds", () => {
		expect(formatTimecode(3723.5)).toBe("01:02:03.500");
	});
});
