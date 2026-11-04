import { create } from "@bufbuild/protobuf";
import { describe, expect, it } from "vitest";

import { imageSupport, NO_SUPPORT, supportOf, UNKNOWN_SUPPORT } from "./support";
import { CapabilitiesSchema, CapabilitiesStatus, GetCapabilitiesResponseSchema } from "~/generated/muxmultitool/system/v1/system_pb";

const full = create(CapabilitiesSchema, {
	encoders: ["gif", "libx264", "libvpx-vp9", "libvpx", "mjpeg", "png"].map((name) => ({ name })),
	filters: ["fps", "scale", "split", "palettegen", "paletteuse"].map((name) => ({ name }))
});

describe("image support", () => {
	it("reads the encoders and filters the features need", () => {
		expect(imageSupport(full)).toEqual({ gifMissing: [], mp4Encoder: "libx264", webmEncoder: "libvpx-vp9", jpeg: true, png: true });
	});

	it("names what a lean build lacks and falls back to VP8", () => {
		const lean = create(CapabilitiesSchema, {
			encoders: [{ name: "libvpx" }, { name: "png" }],
			filters: [{ name: "fps" }, { name: "scale" }]
		});
		expect(imageSupport(lean)).toEqual({
			gifMissing: ["gif", "split", "palettegen", "paletteuse"],
			mp4Encoder: undefined,
			webmEncoder: "libvpx",
			jpeg: false,
			png: true
		});
	});

	it("allows everything until the check answers and nothing without ffmpeg", () => {
		expect(supportOf(undefined)).toBe(UNKNOWN_SUPPORT);
		expect(supportOf(create(GetCapabilitiesResponseSchema, { status: CapabilitiesStatus.UNAVAILABLE }))).toBe(NO_SUPPORT);
		expect(supportOf(create(GetCapabilitiesResponseSchema, { status: CapabilitiesStatus.AVAILABLE, capabilities: full })).jpeg).toBe(true);
	});
});
