import { create } from "@bufbuild/protobuf";
import { describe, expect, it } from "vitest";

import {
	canCreateGif,
	gifName,
	gifRange,
	gifVideoName,
	hasExtension,
	isGifSource,
	toGifOperation,
	toGifToVideoOperation,
	videoEncoderFor
} from "./gif";
import { NO_SUPPORT, UNKNOWN_SUPPORT } from "./support";
import { FileReportSchema } from "~/generated/muxmultitool/files/v1/files_pb";
import { EstimateGifResponseSchema, GifLimit, GifQuality, GifVideoFormat } from "~/generated/muxmultitool/gif/v1/gif_pb";

describe("gif form", () => {
	it("takes the range from the player and caps a fresh one at the duration limit", () => {
		expect(gifRange({}, 60, 10)).toEqual({ startMs: 0n, endMs: 10_000n });
		expect(gifRange({}, 4, 10)).toEqual({ startMs: 0n, endMs: 4000n });
		expect(gifRange({ start: 12 }, 60, 10)).toEqual({ startMs: 12_000n, endMs: 22_000n });
		expect(gifRange({ start: 12, end: 40 }, 60, 10)).toEqual({ startMs: 12_000n, endMs: 40_000n });
		expect(gifRange({ end: 5 }, 0, 10)).toEqual({ startMs: 0n, endMs: 5000n });
		expect(gifRange({ start: 9, end: 3 }, 60, 10)).toEqual({ startMs: 9000n, endMs: 9000n });
	});

	it("builds the operations", () => {
		const operation = toGifOperation({
			input: "in",
			output: " clip.gif ",
			startMs: 1000n,
			endMs: 5000n,
			fps: 12,
			quality: GifQuality.BEST,
			loop: false,
			maxDurationS: 10,
			maxWidth: 480,
			maxBytes: 8_000_000
		});
		expect(operation.output).toBe("clip.gif");
		expect(operation.width).toBe(480);
		expect(operation.playOnce).toBe(true);
		expect(operation.limits).toMatchObject({ maxDurationMs: 10_000n, maxWidth: 480, maxBytes: 8_000_000n });
		expect(toGifToVideoOperation("in", "out.webm", GifVideoFormat.WEBM)).toMatchObject({
			input: "in",
			output: "out.webm",
			format: GifVideoFormat.WEBM
		});
	});

	it("knows a GIF source and names the outputs", () => {
		expect(isGifSource(create(FileReportSchema, { container: { format: "gif" } }))).toBe(true);
		expect(isGifSource(create(FileReportSchema, { container: { format: "matroska,webm" } }))).toBe(false);
		expect(isGifSource(undefined)).toBe(false);
		expect(gifName("clip.mkv")).toBe("clip.gif");
		expect(gifVideoName("clip.gif", GifVideoFormat.MP4)).toBe("clip.mp4");
		expect(gifVideoName("clip.gif", GifVideoFormat.WEBM)).toBe("clip.webm");
		expect(hasExtension("a.PNG", ["png", "jpg"])).toBe(true);
		expect(hasExtension("a.gif", ["png", "jpg"])).toBe(false);
	});

	it("starts only within the limits on a build that can", () => {
		const fine = create(EstimateGifResponseSchema, { violations: [] });
		const over = create(EstimateGifResponseSchema, { violations: [{ limit: GifLimit.SIZE, message: "too big" }] });
		expect(canCreateGif(fine, UNKNOWN_SUPPORT)).toBe(true);
		expect(canCreateGif(over, UNKNOWN_SUPPORT)).toBe(false);
		expect(canCreateGif(undefined, UNKNOWN_SUPPORT)).toBe(false);
		expect(canCreateGif(fine, NO_SUPPORT)).toBe(false);
		expect(videoEncoderFor(UNKNOWN_SUPPORT, GifVideoFormat.WEBM)).toBe("libvpx-vp9");
		expect(videoEncoderFor(NO_SUPPORT, GifVideoFormat.MP4)).toBeUndefined();
	});
});
