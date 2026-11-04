import { describe, expect, it } from "vitest";

import { defaultSplitForm, MAX_PARTS, sizeToBytes, type SplitForm, splitMode } from "./split";

const form = (patch: Partial<SplitForm>): SplitForm => ({ ...defaultSplitForm(), ...patch });

describe("split form", () => {
	it("converts sizes to bytes", () => {
		expect(sizeToBytes(4, "GB")).toBe(4_000_000_000n);
		expect(sizeToBytes(1.5, "MB")).toBe(1_500_000n);
		expect(sizeToBytes(0, "GB")).toBeUndefined();
		expect(sizeToBytes(Number.NaN, "GB")).toBeUndefined();
	});

	it("builds the duration mode from a typed time", () => {
		expect(splitMode(form({ by: "duration", duration: "10:00" }))).toEqual({ mode: { case: "maxDurationSeconds", value: 600 } });
		expect(splitMode(form({ by: "duration", duration: "90" }))).toEqual({ mode: { case: "maxDurationSeconds", value: 90 } });
		expect(splitMode(form({ by: "duration", duration: "0" }))).toEqual({ error: "duration" });
		expect(splitMode(form({ by: "duration", duration: "abc" }))).toEqual({ error: "duration" });
	});

	it("builds the size mode", () => {
		expect(splitMode(form({ by: "size", size: 4, sizeUnit: "GB" }))).toEqual({ mode: { case: "maxSizeBytes", value: 4_000_000_000n } });
		expect(splitMode(form({ by: "size", size: -1 }))).toEqual({ error: "size" });
	});

	it("builds the count mode for 2 to 1000 whole parts", () => {
		expect(splitMode(form({ by: "count", count: 3 }))).toEqual({ mode: { case: "parts", value: 3 } });
		expect(splitMode(form({ by: "count", count: MAX_PARTS }))).toEqual({ mode: { case: "parts", value: MAX_PARTS } });
		expect(splitMode(form({ by: "count", count: 1 }))).toEqual({ error: "count" });
		expect(splitMode(form({ by: "count", count: 2.5 }))).toEqual({ error: "count" });
		expect(splitMode(form({ by: "count", count: MAX_PARTS + 1 }))).toEqual({ error: "count" });
	});
});
