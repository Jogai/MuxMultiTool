import type { SubtitleCue } from "~/generated/muxmultitool/media/v1/media_pb";
import { StreamType, type StreamSummary } from "~/generated/muxmultitool/files/v1/files_pb";

/** Text subtitle codecs the media service can read; anything else is image-based and refused. */
const TEXT_SUBTITLE_CODECS = new Set(["subrip", "srt", "ass", "ssa", "webvtt", "vtt", "mov_text", "text", "tx3g", "ttml", "microdvd"]);

export function isTextSubtitle(codec: string): boolean {
	return TEXT_SUBTITLE_CODECS.has(codec.toLowerCase());
}

/** The subtitle streams of a report, in container order. */
export function subtitleStreams(streams: readonly StreamSummary[]): StreamSummary[] {
	return streams.filter((stream) => stream.type === StreamType.SUBTITLE);
}

/** Cues ordered by start time, which `activeCues` needs. Stable, so cues that start together keep file order. */
export function sortCues(cues: readonly SubtitleCue[]): SubtitleCue[] {
	return [...cues].sort((a, b) => a.startSeconds - b.startSeconds);
}

/** Longer cues than this are not expected; it bounds the backwards scan for overlapping cues. */
const MAX_CUE_SECONDS = 60;

/** The cues that show at `seconds`, in start order. `cues` must be sorted by start. */
export function activeCues(cues: readonly SubtitleCue[], seconds: number): SubtitleCue[] {
	// Last cue that has started
	let low = 0;
	let high = cues.length;
	while (low < high) {
		const mid = (low + high) >>> 1;
		if (cues[mid]!.startSeconds <= seconds) {
			low = mid + 1;
		} else {
			high = mid;
		}
	}
	const active: SubtitleCue[] = [];
	for (let index = low - 1; index >= 0; index -= 1) {
		const cue = cues[index]!;
		if (cue.startSeconds < seconds - MAX_CUE_SECONDS) {
			break;
		}
		if (cue.endSeconds > seconds) {
			active.unshift(cue);
		}
	}
	return active;
}
