import { create } from "@bufbuild/protobuf";

import { extensionOf, stem } from "~/core/libs/fileNames";

import { toMs } from "./outputs";
import type { ImageSupport } from "./support";
import type { PlayerRange } from "~/features/player/lib/range";
import type { FileReport } from "~/generated/muxmultitool/files/v1/files_pb";
import {
	type EstimateGifResponse,
	type GifQuality,
	type GifToVideoOperation,
	GifToVideoOperationSchema,
	GifVideoFormat,
	type VideoToGifOperation,
	VideoToGifOperationSchema
} from "~/generated/muxmultitool/gif/v1/gif_pb";

/** The limits the user states (sketch 08): the server picks scale and palette options within them. */
export const DURATION_LIMITS_S = [3, 5, 10, 15, 30] as const;
export const WIDTH_LIMITS = [240, 320, 480, 640, 800] as const;
export const FPS_OPTIONS = [5, 10, 12, 15, 20, 25] as const;
export const SIZE_LIMITS_MB = [1, 2, 4, 8, 16] as const;

/** What the GIF panel holds. The range comes from the player; everything else is a control. */
export interface GifForm {
	input: string;
	/** A bare file name next to the input, or an absolute path; empty names `<input>.gif`. */
	output: string;
	startMs: bigint;
	endMs: bigint;
	fps: number;
	quality: GifQuality;
	loop: boolean;
	maxDurationS: number;
	maxWidth: number;
	maxBytes: number;
}

/**
 * The range a GIF is made from: the player's range, with the clip's start and end standing in for
 * an unset side and the end capped at the duration limit from the start, so a fresh file shows the
 * first seconds instead of a violation.
 */
export function gifRange(range: PlayerRange, duration: number, maxDurationS: number): { startMs: bigint; endMs: bigint } {
	const start = Math.max(0, range.start ?? 0);
	const limit = start + maxDurationS;
	const end = range.end ?? Math.min(limit, duration || limit);
	return { startMs: toMs(start), endMs: toMs(Math.max(end, start)) };
}

/** The operation the estimate and the submit both take; the width is the width limit. */
export function toGifOperation(form: GifForm): VideoToGifOperation {
	return create(VideoToGifOperationSchema, {
		input: form.input,
		output: form.output.trim(),
		startMs: form.startMs,
		endMs: form.endMs,
		fps: form.fps,
		width: form.maxWidth,
		quality: form.quality,
		playOnce: !form.loop,
		limits: { maxDurationMs: BigInt(form.maxDurationS * 1000), maxWidth: form.maxWidth, maxBytes: BigInt(form.maxBytes) }
	});
}

export function toGifToVideoOperation(input: string, output: string, format: GifVideoFormat): GifToVideoOperation {
	return create(GifToVideoOperationSchema, { input, output: output.trim(), format });
}

/** Whether the file is a GIF, so the panel offers GIF → video instead of video → GIF. */
export function isGifSource(report: Pick<FileReport, "container"> | undefined): boolean {
	return report?.container?.format.split(",").includes("gif") ?? false;
}

/** `clip.gif` for `clip.mkv`. */
export function gifName(fileName: string): string {
	return `${stem(fileName)}.gif`;
}

/** `clip.mp4` or `clip.webm` for `clip.gif`. */
export function gifVideoName(fileName: string, format: GifVideoFormat): string {
	return `${stem(fileName)}.${format === GifVideoFormat.WEBM ? "webm" : "mp4"}`;
}

/** The encoder a GIF → video format would use, or nothing when the build lacks one. */
export function videoEncoderFor(support: ImageSupport, format: GifVideoFormat): string | undefined {
	return format === GifVideoFormat.WEBM ? support.webmEncoder : support.mp4Encoder;
}

/** Whether the GIF may be made: the build can, and the estimate for the current form is within every limit. */
export function canCreateGif(estimate: EstimateGifResponse | undefined, support: ImageSupport): boolean {
	return support.gifMissing.length === 0 && estimate !== undefined && estimate.violations.length === 0;
}

/** Whether the output's extension matches what the job writes; the server refuses another extension for a snapshot, not here. */
export function hasExtension(output: string, extensions: readonly string[]): boolean {
	return extensions.includes(extensionOf(output).toLowerCase());
}
