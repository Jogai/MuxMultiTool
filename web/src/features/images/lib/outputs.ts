import { baseName, joinPath } from "~/core/libs/filePicker";
import { formatTimecode } from "~/core/libs/format";

/**
 * Where a job's output lands and how the browser gets at it afterwards.
 *
 * The operations name their output as a bare file name next to the input (or an absolute path). In
 * container mode the browser then downloads it from `GET /files/{id}`, which needs the output's file
 * id. The server's ids are opaque to clients but not secret (crates/core/src/files/id.rs: base64url
 * over `r<root>:<a/b/c>` or `a:<absolute path>`), and the server checks every decoded id against the
 * media roots, so the frontend can derive the id of a file next to the input without a round trip.
 */
export type FileLocation = { kind: "root"; root: number; names: readonly string[] } | { kind: "absolute"; path: string };

const encoder = new TextEncoder();
const decoder = new TextDecoder();

function fromBase64Url(text: string): string | undefined {
	const padded = text
		.replaceAll("-", "+")
		.replaceAll("_", "/")
		.padEnd(Math.ceil(text.length / 4) * 4, "=");
	try {
		const binary = atob(padded);
		return decoder.decode(Uint8Array.from(binary, (char) => char.codePointAt(0) ?? 0));
	} catch {
		return undefined;
	}
}

function toBase64Url(text: string): string {
	const binary = Array.from(encoder.encode(text), (byte) => String.fromCodePoint(byte)).join("");
	return btoa(binary).replaceAll("+", "-").replaceAll("/", "_").replace(/=+$/, "");
}

/** The location behind a file id, or nothing when the id is not one the server made. */
export function decodeFileId(id: string): FileLocation | undefined {
	const text = fromBase64Url(id);
	if (text === undefined) {
		return undefined;
	}
	if (text.startsWith("a:")) {
		return { kind: "absolute", path: text.slice(2) };
	}
	const match = /^r(\d+):(.*)$/s.exec(text);
	if (!match) {
		return undefined;
	}
	const names = match[2] === "" ? [] : (match[2] ?? "").split("/");
	if (names.some((name) => name === "" || name === "." || name === "..")) {
		return undefined;
	}
	return { kind: "root", root: Number(match[1]), names };
}

export function encodeFileId(location: FileLocation): string {
	return toBase64Url(location.kind === "root" ? `r${location.root}:${location.names.join("/")}` : `a:${location.path}`);
}

/** True for a file name without a folder part. */
export function isBareName(output: string): boolean {
	return output !== "" && !output.includes("/") && !output.includes("\\");
}

/** `movie` for `movie.mkv`, the whole name when there is no extension. */
export function stem(name: string): string {
	const dot = name.lastIndexOf(".");
	return dot > 0 ? name.slice(0, dot) : name;
}

/** `mkv` for `movie.mkv`, empty when there is no extension. */
export function extensionOf(name: string): string {
	const dot = name.lastIndexOf(".");
	return dot > 0 ? name.slice(dot + 1) : "";
}

/**
 * The file id of `name` next to the input, for the download link. The input is a file id from the
 * files service or, in desktop mode, an absolute path from a native dialog; an id of the absolute
 * form gives an id of the same form, which the download route does not serve but the desktop app
 * does not need. Nothing for an output that is not a bare name.
 */
export function siblingFileId(inputId: string, name: string): string | undefined {
	if (!isBareName(name)) {
		return undefined;
	}
	const location = decodeFileId(inputId);
	if (location?.kind === "root") {
		if (location.names.length === 0) {
			return undefined;
		}
		return encodeFileId({ kind: "root", root: location.root, names: [...location.names.slice(0, -1), name] });
	}
	const path = location?.kind === "absolute" ? location.path : inputId;
	const folder = path.slice(0, path.length - baseName(path).length);
	return folder === "" ? undefined : encodeFileId({ kind: "absolute", path: joinPath(folder, name) });
}

/** The download URL of a file in container mode, or nothing when the id is not one the route serves. */
export function downloadUrl(fileId: string | undefined): string | undefined {
	if (fileId === undefined || decodeFileId(fileId)?.kind !== "root") {
		return undefined;
	}
	return `/files/${encodeURIComponent(fileId)}`;
}

/** The frame route for a position, as the snapshot preview shows it. */
export function frameUrl(fileId: string, seconds: number, format: "jpg" | "png" = "jpg"): string {
	return `/media/${encodeURIComponent(fileId)}/frame?t=${Math.max(0, seconds).toFixed(3)}&format=${format}`;
}

/**
 * The output path of a job on the server: the last argument of its command. Every operation here
 * runs one ffmpeg command that ends with its output; a job of several commands (`&&`) has no single
 * output.
 */
export function jobOutputPath(job: { argv: readonly string[] }): string | undefined {
	const last = job.argv.at(-1);
	if (last === undefined || last === "&&" || job.argv.includes("&&")) {
		return undefined;
	}
	return last;
}

/** `movie.00-12-34.500.png` for a snapshot of `movie.mkv` at 12:34.5. */
export function snapshotName(fileName: string, seconds: number, format: "png" | "jpg"): string {
	return `${stem(fileName)}.${formatTimecode(seconds).replaceAll(":", "-")}.${format}`;
}

/** `movie.cover.mkv` for `movie.mkv`: the same container keeps every stream on the copy. */
export function coverName(fileName: string): string {
	const extension = extensionOf(fileName);
	return extension === "" ? `${fileName}.cover` : `${stem(fileName)}.cover.${extension}`;
}

/** `movie.cropped.mkv` for `movie.mkv`, the sketch's template `{name}.cropped.{ext}`. */
export function croppedName(fileName: string): string {
	const extension = extensionOf(fileName);
	return extension === "" ? `${fileName}.cropped` : `${stem(fileName)}.cropped.${extension}`;
}

/** The position in whole milliseconds for the operations. */
export function toMs(seconds: number): bigint {
	return BigInt(Math.max(0, Math.round(seconds * 1000)));
}

/** The link to a job on the Jobs screen. */
export function jobLink(jobId: string): { path: string; query: { job: string } } {
	return { path: "/jobs", query: { job: jobId } };
}
