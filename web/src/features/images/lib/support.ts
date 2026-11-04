import { type Capabilities, CapabilitiesStatus, type GetCapabilitiesResponse } from "~/generated/muxmultitool/system/v1/system_pb";

/**
 * What the ffmpeg build can do for the image features, from the capability check. The names mirror
 * the core crate (gif::missing_for_gif, gif::VideoFormat::encoders, snapshot::ImageFormat::encoder),
 * so the frontend disables the same options the server would refuse. Crop support is per file and
 * comes from CropService.PlanCrop instead.
 */
export interface ImageSupport {
	/** Encoders and filters a video → GIF job needs and the build lacks; empty when it can make GIFs. */
	gifMissing: string[];
	/** The encoder a GIF → MP4 job would use, or nothing when the build has none. */
	mp4Encoder?: string;
	/** The encoder a GIF → WebM job would use (VP9, else VP8), or nothing. */
	webmEncoder?: string;
	/** Whether a JPEG snapshot can be saved (the mjpeg encoder). */
	jpeg: boolean;
	/** Whether a PNG snapshot can be saved (the png encoder). */
	png: boolean;
}

const GIF_ENCODER = "gif";
const GIF_FILTERS = ["fps", "scale", "split", "palettegen", "paletteuse"];
const MP4_ENCODERS = ["libx264"];
const WEBM_ENCODERS = ["libvpx-vp9", "libvpx"];

export function imageSupport(capabilities: Capabilities): ImageSupport {
	const encoders = new Set(capabilities.encoders.map((codec) => codec.name));
	const filters = new Set(capabilities.filters.map((filter) => filter.name));
	const gifMissing = [...(encoders.has(GIF_ENCODER) ? [] : [GIF_ENCODER]), ...GIF_FILTERS.filter((name) => !filters.has(name))];
	return {
		gifMissing,
		mp4Encoder: MP4_ENCODERS.find((name) => encoders.has(name)),
		webmEncoder: WEBM_ENCODERS.find((name) => encoders.has(name)),
		jpeg: encoders.has("mjpeg"),
		png: encoders.has("png")
	};
}

/** Everything allowed: what the screen assumes until the capability check answered. */
export const UNKNOWN_SUPPORT: ImageSupport = { gifMissing: [], mp4Encoder: "libx264", webmEncoder: "libvpx-vp9", jpeg: true, png: true };

/** Nothing allowed: ffmpeg is missing or broken. */
export const NO_SUPPORT: ImageSupport = { gifMissing: [GIF_ENCODER], jpeg: false, png: false };

/** The support a capability response gives: everything until it answers, nothing when ffmpeg is unavailable. */
export function supportOf(response: GetCapabilitiesResponse | undefined): ImageSupport {
	if (response === undefined) {
		return UNKNOWN_SUPPORT;
	}
	if (response.status !== CapabilitiesStatus.AVAILABLE || !response.capabilities) {
		return NO_SUPPORT;
	}
	return imageSupport(response.capabilities);
}
