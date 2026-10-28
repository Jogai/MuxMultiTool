import { describe, expect, it } from "vitest";

import { elapsedMs, formatBytes, formatPercent, formatSpan, formatSpeed } from "./jobFormat";

describe("formatSpan", () => {
	it("shows minutes and seconds, and hours only when needed", () => {
		expect(formatSpan(72_000)).toBe("1:12");
		expect(formatSpan(400_000n)).toBe("6:40");
		expect(formatSpan(3_731_000)).toBe("1:02:11");
		expect(formatSpan(0)).toBe("0:00");
		expect(formatSpan(-5)).toBe("0:00");
	});
});

describe("formatSpeed", () => {
	it("keeps one decimal under ten and none above", () => {
		expect(formatSpeed(3.42)).toBe("3.4×");
		expect(formatSpeed(12.7)).toBe("13×");
		expect(formatSpeed(0)).toBeUndefined();
		expect(formatSpeed(undefined)).toBeUndefined();
	});
});

describe("formatPercent", () => {
	it("rounds and clamps", () => {
		expect(formatPercent(61.6)).toBe("62%");
		expect(formatPercent(104)).toBe("100%");
		expect(formatPercent(undefined)).toBeUndefined();
	});
});

describe("formatBytes", () => {
	it("picks the unit", () => {
		expect(formatBytes(512)).toBe("512 B");
		expect(formatBytes(1_500_000n)).toBe("1.5 MB");
		expect(formatBytes(2_000_000_000)).toBe("2.0 GB");
	});
});

describe("elapsedMs", () => {
	it("measures to the end, or to now while running", () => {
		expect(elapsedMs({ startedUnixMs: 0n, finishedUnixMs: 0n }, 5000)).toBeUndefined();
		expect(elapsedMs({ startedUnixMs: 1000n, finishedUnixMs: 0n }, 5000)).toBe(4000);
		expect(elapsedMs({ startedUnixMs: 1000n, finishedUnixMs: 3000n }, 5000)).toBe(2000);
	});
});
