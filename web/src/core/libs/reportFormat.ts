import { type ContainerInfo, SizeMethod, type StreamSummary, StreamType } from "~/generated/muxmultitool/files/v1/files_pb";

/** The size in GB as the report shows it: decimal gigabytes with two decimals, `24.80`. */
export function formatGb(bytes: bigint | number): string {
	return (Math.max(0, Number(bytes)) / 1e9).toFixed(2);
}

/**
 * The size cell of a stream: `~` marks an estimate, `?` means the server found no source for the size
 * (see `SizeMethod`), so a `0.00` from an unknown size is never mistaken for an empty stream.
 */
export function formatStreamSize(stream: Pick<StreamSummary, "sizeBytes" | "sizeMethod">): string {
	switch (stream.sizeMethod) {
		case SizeMethod.UNSPECIFIED:
			return "?";
		case SizeMethod.ESTIMATED:
			return `~${formatGb(stream.sizeBytes)}`;
		default:
			return formatGb(stream.sizeBytes);
	}
}

/**
 * A duration in seconds as `h:mm:ss` (`2:01:33`, `0:02:21`), as the sketch shows durations and the
 * old script printed them. `undefined` when the duration is unknown (0 or not a finite number).
 */
export function formatDuration(seconds: number): string | undefined {
	if (!Number.isFinite(seconds) || seconds <= 0) {
		return undefined;
	}
	const total = Math.round(seconds);
	const h = Math.floor(total / 3600);
	const m = Math.floor((total % 3600) / 60);
	const s = total % 60;
	const pad = (n: number) => String(n).padStart(2, "0");
	return `${h}:${pad(m)}:${pad(s)}`;
}

/** A bit rate as `4.9 Mb/s` or `192 kb/s`; `undefined` when unknown. */
export function formatBitRate(bitsPerSecond: bigint | number): string | undefined {
	const value = Number(bitsPerSecond);
	if (!Number.isFinite(value) || value <= 0) {
		return undefined;
	}
	if (value >= 1e6) {
		return `${(value / 1e6).toFixed(1)} Mb/s`;
	}
	if (value >= 1e3) {
		return `${Math.round(value / 1e3)} kb/s`;
	}
	return `${Math.round(value)} b/s`;
}

/** The type column: `video`, `audio`, `subtitle`, `data`, `attachment`, or `?` for a type the server does not know. */
export function streamTypeName(type: StreamType): string {
	switch (type) {
		case StreamType.VIDEO:
			return "video";
		case StreamType.AUDIO:
			return "audio";
		case StreamType.SUBTITLE:
			return "subtitle";
		case StreamType.DATA:
			return "data";
		case StreamType.ATTACHMENT:
			return "attachment";
		default:
			return "?";
	}
}

/** Short container names for the file table, keyed by ffprobe's demuxer name. */
const CONTAINER_NAMES: Record<string, string> = {
	"matroska,webm": "MKV",
	"mov,mp4,m4a,3gp,3g2,mj2": "MP4",
	mpegts: "M2TS",
	avi: "AVI",
	webm: "WebM",
	mpeg: "MPEG",
	asf: "WMV",
	flv: "FLV",
	ogg: "OGG",
	wav: "WAV",
	flac: "FLAC",
	mp3: "MP3"
};

/**
 * The container column: `MKV`, `MP4`, … from ffprobe's demuxer name, with the file extension to tell
 * the formats one demuxer serves apart (`.webm` is "matroska,webm" too, `.m4a` is the MP4 demuxer).
 */
export function containerLabel(container: Pick<ContainerInfo, "format">, fileName: string): string {
	const extension = fileName.split(".").at(-1)?.toUpperCase() ?? "";
	if (container.format === "matroska,webm" && extension === "WEBM") {
		return "WebM";
	}
	if (container.format === "mov,mp4,m4a,3gp,3g2,mj2" && ["MOV", "M4A", "M4V", "3GP"].includes(extension)) {
		return extension;
	}
	if (container.format === "mpegts" && extension === "TS") {
		return "TS";
	}
	return CONTAINER_NAMES[container.format] ?? container.format.split(",")[0]?.toUpperCase() ?? "?";
}

/** The video column: the first video stream as `hevc 2160p`, or `undefined` for a file without video. */
export function videoLabel(streams: readonly StreamSummary[]): string | undefined {
	const video = streams.find((stream) => stream.type === StreamType.VIDEO && !stream.flags.includes("attached_pic"));
	if (!video) {
		return undefined;
	}
	const parts = [video.codec];
	if (video.height > 0) {
		parts.push(`${video.height}p`);
	}
	return parts.join(" ");
}

/** A short line for an audio stream's details: `7.1 · 48 kHz · 4.9 Mb/s`. */
export function audioLabel(stream: Pick<StreamSummary, "channels" | "channelLayout" | "sampleRate" | "bitRate">): string | undefined {
	const parts: string[] = [];
	if (stream.channelLayout) {
		parts.push(stream.channelLayout);
	} else if (stream.channels > 0) {
		parts.push(`${stream.channels} ch`);
	}
	if (stream.sampleRate > 0) {
		parts.push(`${(stream.sampleRate / 1000).toLocaleString("en-US", { maximumFractionDigits: 1 })} kHz`);
	}
	const rate = formatBitRate(stream.bitRate);
	if (rate) {
		parts.push(rate);
	}
	return parts.length > 0 ? parts.join(" · ") : undefined;
}
