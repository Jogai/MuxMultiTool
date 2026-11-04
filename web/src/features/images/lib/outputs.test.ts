import { describe, expect, it } from "vitest";

import {
	coverName,
	croppedName,
	decodeFileId,
	downloadUrl,
	encodeFileId,
	frameUrl,
	isBareName,
	jobOutputPath,
	siblingFileId,
	snapshotName,
	toMs
} from "./outputs";

/** base64url of `r0:series/s01/e01.mkv`, as the server would issue it. */
const ROOT_ID = "cjA6c2VyaWVzL3MwMS9lMDEubWt2";
/** base64url of `a:/home/me/clip.mp4`, a native-dialog path in desktop mode. */
const ABSOLUTE_ID = "YTovaG9tZS9tZS9jbGlwLm1wNA";

describe("file ids", () => {
	it("round-trips the server's two forms, including non-ASCII names", () => {
		expect(decodeFileId(ROOT_ID)).toEqual({ kind: "root", root: 0, names: ["series", "s01", "e01.mkv"] });
		expect(decodeFileId(ABSOLUTE_ID)).toEqual({ kind: "absolute", path: "/home/me/clip.mp4" });
		const umlaut = { kind: "root", root: 1, names: ["with space and ümlaut.mp4"] } as const;
		expect(decodeFileId(encodeFileId(umlaut))).toEqual(umlaut);
		expect(encodeFileId({ kind: "root", root: 0, names: [] })).toBe("cjA6");
	});

	it("rejects what the server did not make", () => {
		expect(decodeFileId("not base64!")).toBeUndefined();
		expect(decodeFileId(encodeFileId({ kind: "absolute", path: "" }).slice(0, 1))).toBeUndefined();
		expect(decodeFileId(btoa("z:a"))).toBeUndefined();
		expect(decodeFileId(btoa("r0:a/../b"))).toBeUndefined();
		expect(decodeFileId("/plain/path.mkv")).toBeUndefined();
	});

	it("names a file next to the input", () => {
		expect(decodeFileId(siblingFileId(ROOT_ID, "e01.gif") ?? "")).toEqual({ kind: "root", root: 0, names: ["series", "s01", "e01.gif"] });
		expect(decodeFileId(siblingFileId(ABSOLUTE_ID, "clip.gif") ?? "")).toEqual({ kind: "absolute", path: "/home/me/clip.gif" });
		expect(decodeFileId(siblingFileId("/home/me/clip.mp4", "clip.gif") ?? "")).toEqual({ kind: "absolute", path: "/home/me/clip.gif" });
		expect(decodeFileId(siblingFileId("C:\\Videos\\clip.mp4", "clip.gif") ?? "")).toEqual({
			kind: "absolute",
			path: "C:\\Videos\\clip.gif"
		});
		expect(siblingFileId(ROOT_ID, "/abs/out.gif")).toBeUndefined();
		expect(siblingFileId("cjA6", "out.gif")).toBeUndefined();
		expect(siblingFileId("clip.mp4", "out.gif")).toBeUndefined();
	});

	it("links downloads only for files under a media root", () => {
		expect(downloadUrl(ROOT_ID)).toBe(`/files/${ROOT_ID}`);
		expect(downloadUrl(ABSOLUTE_ID)).toBeUndefined();
		expect(downloadUrl(undefined)).toBeUndefined();
	});
});

describe("output naming", () => {
	it("tells bare names from paths", () => {
		expect(isBareName("out.gif")).toBe(true);
		expect(isBareName("")).toBe(false);
		expect(isBareName("a/out.gif")).toBe(false);
		expect(isBareName("a\\out.gif")).toBe(false);
	});

	it("names the outputs after the input", () => {
		expect(snapshotName("movie.mkv", 754.5, "png")).toBe("movie.00-12-34.500.png");
		expect(coverName("movie.mkv")).toBe("movie.cover.mkv");
		expect(coverName("movie")).toBe("movie.cover");
		expect(croppedName("Movie A (2024).mkv")).toBe("Movie A (2024).cropped.mkv");
		expect(toMs(1.2345)).toBe(1235n);
		expect(toMs(-1)).toBe(0n);
	});

	it("builds the frame URL and reads a job's output path", () => {
		expect(frameUrl("id 1", 12.5)).toBe("/media/id%201/frame?t=12.500&format=jpg");
		expect(frameUrl("x", -1, "png")).toBe("/media/x/frame?t=0.000&format=png");
		expect(jobOutputPath({ argv: ["ffmpeg", "-i", "in.mkv", "/out/clip.gif"] })).toBe("/out/clip.gif");
		expect(jobOutputPath({ argv: ["ffmpeg", "a", "&&", "ffmpeg", "b"] })).toBeUndefined();
		expect(jobOutputPath({ argv: [] })).toBeUndefined();
	});
});
