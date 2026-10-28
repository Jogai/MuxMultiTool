import { create } from "@bufbuild/protobuf";
import { describe, expect, it } from "vitest";

import { alignColumns, containerLine, reportAsText } from "./reportText";
import { EntryKind, FileReportSchema, SizeMethod, StreamType } from "~/generated/muxmultitool/files/v1/files_pb";

const movieA = create(FileReportSchema, {
	fileId: "a",
	entry: { id: "a", name: "Movie A (2024).mkv", kind: EntryKind.FILE, sizeBytes: 24_800_000_000n },
	container: { format: "matroska,webm", durationSeconds: 7293, sizeBytes: 24_800_000_000n },
	streams: [
		{
			index: 0,
			type: StreamType.VIDEO,
			codec: "hevc",
			language: "und",
			sizeBytes: 21_950_000_000n,
			sizeMethod: SizeMethod.TAGS,
			flags: ["default"]
		},
		{
			index: 1,
			type: StreamType.AUDIO,
			codec: "truehd",
			language: "eng",
			title: "Atmos 7.1",
			sizeBytes: 2_310_000_000n,
			sizeMethod: SizeMethod.TAGS
		},
		{
			index: 2,
			type: StreamType.SUBTITLE,
			codec: "hdmv_pgs_subtitle",
			language: "fre",
			sizeBytes: 0n,
			sizeMethod: SizeMethod.UNSPECIFIED,
			flags: ["forced"]
		}
	]
});

const broken = create(FileReportSchema, {
	fileId: "b",
	entry: { id: "b", name: "broken.mp4", kind: EntryKind.FILE },
	error: { message: "the input is not a media file ffmpeg can read", ffprobeOutput: "moov atom not found" }
});

describe("alignColumns", () => {
	it("pads every column but the last to its widest cell", () => {
		expect(
			alignColumns([
				["a", "bb", "c"],
				["dddd", "e", ""]
			])
		).toEqual(["a     bb  c", "dddd  e"]);
	});
});

describe("containerLine", () => {
	it("joins container, duration and size", () => {
		expect(containerLine(movieA)).toBe("MKV · 2:01:33 · 24.80 GB");
		expect(containerLine(broken)).toBe("");
	});
});

describe("reportAsText", () => {
	it("prints one aligned table per file when grouped, with failures in place", () => {
		expect(reportAsText([movieA, broken], "grouped")).toBe(
			[
				"Movie A (2024).mkv  (MKV · 2:01:33 · 24.80 GB)",
				"  #   Type      Codec              Lang  Title      Size (GB)  Flags",
				"  #0  video     hevc               und              21.95      default",
				"  #1  audio     truehd             eng   Atmos 7.1  2.31",
				"  #2  subtitle  hdmv_pgs_subtitle  fre              ?          forced",
				"",
				"broken.mp4",
				"  error: the input is not a media file ffmpeg can read"
			].join("\n")
		);
	});

	it("prints one table with a file column when flat, failures after it", () => {
		const text = reportAsText([movieA, broken], "flat");
		const lines = text.split("\n");
		expect(lines[0]).toBe("File                #   Type      Codec              Lang  Title      Size (GB)  Flags");
		expect(lines[1]).toBe("Movie A (2024).mkv  #0  video     hevc               und              21.95      default");
		expect(lines).toHaveLength(5);
		expect(lines.at(-1)).toBe("broken.mp4: the input is not a media file ffmpeg can read");
	});

	it("names a vanished file by its id", () => {
		const gone = create(FileReportSchema, { fileId: "gone-id", error: { message: "not found" } });
		expect(reportAsText([gone], "grouped")).toBe("gone-id\n  error: not found");
	});
});
