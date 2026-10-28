import { describe, expect, it } from "vitest";

import { choosePlaybackEngine, HLS_MIME_TYPE, PREVIEW_MSE_TYPE } from "./engine";

describe("choosePlaybackEngine", () => {
	it("uses hls.js wherever MediaSource decodes H.264/AAC, whatever canPlayType says", () => {
		const engine = choosePlaybackEngine({
			mediaSource: { isTypeSupported: (type) => type === PREVIEW_MSE_TYPE },
			// Chromium 156 answers "maybe" with its own HLS player; not relied on
			canPlayType: () => "maybe"
		});
		expect(engine).toBe("hls.js");
	});

	it("falls back to the native player only where MSE is absent", () => {
		expect(choosePlaybackEngine({ canPlayType: (type) => (type === HLS_MIME_TYPE ? "maybe" : "") })).toBe("native");
		expect(
			choosePlaybackEngine({
				mediaSource: { isTypeSupported: () => false },
				canPlayType: (type) => (type === HLS_MIME_TYPE ? "probably" : "")
			})
		).toBe("native");
	});

	it("reports no engine when neither works, such as Firefox on Linux without libavcodec", () => {
		expect(choosePlaybackEngine({ mediaSource: { isTypeSupported: () => false }, canPlayType: () => "" })).toBe("none");
	});
});
