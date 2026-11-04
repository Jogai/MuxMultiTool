import type { PlanSplitRequest } from "~/generated/muxmultitool/cut/v1/split_pb";
import { parseTimecode } from "~/features/player/lib/timecode";

/** The three ways to split (sketch 04, "Split…"). */
export type SplitBy = "duration" | "size" | "count";

/** What the split dialog holds. */
export interface SplitForm {
	by: SplitBy;
	/** A typed duration (`MM:SS`, `1:30:00`, `600`) for `duration`. */
	duration: string;
	/** A size for `size`, in `sizeUnit`. */
	size: number;
	sizeUnit: SizeUnit;
	/** The number of parts for `count`. */
	count: number;
}

export type SizeUnit = "MB" | "GB";

/** The most parts `PlanSplit` accepts. */
export const MAX_PARTS = 1000;

/** The validation problems a split form can have; the keys of `cut.split.errors`. */
export type SplitError = "duration" | "size" | "count";

const BYTES: Record<SizeUnit, number> = { MB: 1_000_000, GB: 1_000_000_000 };

export function defaultSplitForm(): SplitForm {
	return { by: "size", duration: "10:00", size: 4, sizeUnit: "GB", count: 2 };
}

/** The size in bytes, or `undefined` for a size that is not positive. */
export function sizeToBytes(size: number, unit: SizeUnit): bigint | undefined {
	if (!Number.isFinite(size) || size <= 0) {
		return undefined;
	}
	return BigInt(Math.round(size * BYTES[unit]));
}

/** The request's `mode` for the form, or the field that is wrong. */
export function splitMode(form: SplitForm): { mode: PlanSplitRequest["mode"] } | { error: SplitError } {
	switch (form.by) {
		case "duration": {
			const seconds = parseTimecode(form.duration);
			return seconds !== undefined && seconds > 0 ? { mode: { case: "maxDurationSeconds", value: seconds } } : { error: "duration" };
		}
		case "size": {
			const bytes = sizeToBytes(form.size, form.sizeUnit);
			return bytes === undefined ? { error: "size" } : { mode: { case: "maxSizeBytes", value: bytes } };
		}
		case "count": {
			const parts = Math.trunc(form.count);
			return Number.isFinite(parts) && parts >= 2 && parts <= MAX_PARTS && parts === form.count
				? { mode: { case: "parts", value: parts } }
				: { error: "count" };
		}
	}
}
