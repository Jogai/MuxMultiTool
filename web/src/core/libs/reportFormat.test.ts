import { create } from "@bufbuild/protobuf";
import { describe, expect, it } from "vitest";

import {
	audioLabel,
	containerLabel,
	formatBitRate,
	formatDuration,
	formatGb,
	formatStreamSize,
	streamTypeName,
	videoLabel
} from "./reportFormat";
import { SizeMethod, StreamSummarySchema, StreamType } from "~/generated/muxmultitool/files/v1/files_pb";

describe("formatGb", () => {
	it("shows decimal gigabytes with two decimals", () => {
		expect(formatGb(24_800_000_000n)).toBe("24.80");
		expect(formatGb(21_950_123_456)).toBe("21.95");
	});

	it("rounds small sizes to 0.00 and never goes negative", () => {
		expect(formatGb(1_000_000)).toBe("0.00");
		expect(formatGb(-5)).toBe("0.00");
	});
});

describe("formatStreamSize", () => {
	it("marks estimates and unknown sizes", () => {
		expect(formatStreamSize({ sizeBytes: 2_310_000_000n, sizeMethod: SizeMethod.TAGS })).toBe("2.31");
		expect(formatStreamSize({ sizeBytes: 2_310_000_000n, sizeMethod: SizeMethod.PACKETS })).toBe("2.31");
		expect(formatStreamSize({ sizeBytes: 2_310_000_000n, sizeMethod: SizeMethod.ESTIMATED })).toBe("~2.31");
		expect(formatStreamSize({ sizeBytes: 0n, sizeMethod: SizeMethod.UNSPECIFIED })).toBe("?");
	});
});

describe("formatDuration", () => {
	it("formats as h:mm:ss", () => {
		expect(formatDuration(7293)).toBe("2:01:33");
		expect(formatDuration(141)).toBe("0:02:21");
		expect(formatDuration(3599.6)).toBe("1:00:00");
	});

	it("is undefined for an unknown duration", () => {
		expect(formatDuration(0)).toBeUndefined();
		expect(formatDuration(Number.NaN)).toBeUndefined();
		expect(formatDuration(-1)).toBeUndefined();
	});
});

describe("formatBitRate", () => {
	it("picks the unit", () => {
		expect(formatBitRate(4_900_000n)).toBe("4.9 Mb/s");
		expect(formatBitRate(192_000)).toBe("192 kb/s");
		expect(formatBitRate(800)).toBe("800 b/s");
		expect(formatBitRate(0n)).toBeUndefined();
	});
});

describe("streamTypeName", () => {
	it("names every known type and marks the rest", () => {
		expect(streamTypeName(StreamType.VIDEO)).toBe("video");
		expect(streamTypeName(StreamType.AUDIO)).toBe("audio");
		expect(streamTypeName(StreamType.SUBTITLE)).toBe("subtitle");
		expect(streamTypeName(StreamType.DATA)).toBe("data");
		expect(streamTypeName(StreamType.ATTACHMENT)).toBe("attachment");
		expect(streamTypeName(StreamType.UNSPECIFIED)).toBe("?");
	});
});

describe("containerLabel", () => {
	it("maps demuxer names and tells formats sharing a demuxer apart", () => {
		expect(containerLabel({ format: "matroska,webm" }, "Movie A (2024).mkv")).toBe("MKV");
		expect(containerLabel({ format: "matroska,webm" }, "Trailer D.webm")).toBe("WebM");
		expect(containerLabel({ format: "mov,mp4,m4a,3gp,3g2,mj2" }, "Movie D.mp4")).toBe("MP4");
		expect(containerLabel({ format: "mov,mp4,m4a,3gp,3g2,mj2" }, "clip.MOV")).toBe("MOV");
		expect(containerLabel({ format: "mpegts" }, "Movie E.m2ts")).toBe("M2TS");
		expect(containerLabel({ format: "mpegts" }, "stream.ts")).toBe("TS");
		expect(containerLabel({ format: "yuv4mpegpipe" }, "raw.y4m")).toBe("YUV4MPEGPIPE");
	});
});

describe("videoLabel", () => {
	it("describes the first real video stream", () => {
		const cover = create(StreamSummarySchema, { index: 0, type: StreamType.VIDEO, codec: "mjpeg", height: 600, flags: ["attached_pic"] });
		const video = create(StreamSummarySchema, { index: 1, type: StreamType.VIDEO, codec: "hevc", height: 2160 });
		const audio = create(StreamSummarySchema, { index: 2, type: StreamType.AUDIO, codec: "aac" });
		expect(videoLabel([cover, video, audio])).toBe("hevc 2160p");
		expect(videoLabel([audio])).toBeUndefined();
		expect(videoLabel([create(StreamSummarySchema, { type: StreamType.VIDEO, codec: "h264" })])).toBe("h264");
	});
});

describe("audioLabel", () => {
	it("joins layout, sample rate and bit rate", () => {
		expect(audioLabel({ channels: 8, channelLayout: "7.1", sampleRate: 48_000, bitRate: 4_900_000n })).toBe("7.1 · 48 kHz · 4.9 Mb/s");
		expect(audioLabel({ channels: 2, channelLayout: "", sampleRate: 44_100, bitRate: 0n })).toBe("2 ch · 44.1 kHz");
		expect(audioLabel({ channels: 0, channelLayout: "", sampleRate: 0, bitRate: 0n })).toBeUndefined();
	});
});
