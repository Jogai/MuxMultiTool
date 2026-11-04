import { create } from "@bufbuild/protobuf";

import { outputPath, renderTemplate } from "./streams";
import { type CombineOperation, CombineOperationSchema } from "~/generated/muxmultitool/combine/v1/combine_pb";

/**
 * The state of the Combine screen (sketch 03): sources on the left, labelled A, B, C, … in order,
 * and the output tracks on the right, each pointing at a stream of a source and carrying the
 * metadata edits that become `-metadata`/`-disposition` options.
 */

export interface SourceDraft {
	/** The file id from FilesService. */
	id: string;
	name: string;
	/** The time shift of every track from this source, as typed; seconds, empty for none. */
	offset: string;
}

/** One output track. A metadata field left `undefined` keeps what the source stream has. */
export interface TrackDraft {
	/** The position of the stream's file in `sources`. */
	source: number;
	streamIndex: number;
	language?: string;
	title?: string;
	default?: boolean;
	forced?: boolean;
}

export interface CombineForm {
	sources: readonly SourceDraft[];
	tracks: readonly TrackDraft[];
	/** The extension that picks the target container, e.g. `mkv`. */
	container: string;
	/** An absolute folder inside a media root; empty puts the output next to the first source. */
	folder: string;
	/** The output file name; `{name}` is the first source's name without extension, `{ext}` the container's. */
	template: string;
}

export const DEFAULT_COMBINE_TEMPLATE = "{name}.combined.{ext}";

export function emptyCombineForm(): CombineForm {
	return { sources: [], tracks: [], container: "mkv", folder: "", template: DEFAULT_COMBINE_TEMPLATE };
}

/** The source's label in the sketch and the backend's messages: A, B, …, Z, AA, … */
export function sourceLabel(index: number): string {
	let label = "";
	let n = index;
	do {
		label = String.fromCodePoint(65 + (n % 26)) + label;
		n = Math.floor(n / 26) - 1;
	} while (n >= 0);
	return label;
}

/** The origin of a track as the sketch shows it, e.g. `B:#1`. */
export function originOf(track: Pick<TrackDraft, "source" | "streamIndex">): string {
	return `${sourceLabel(track.source)}:#${track.streamIndex}`;
}

/** A plain decimal number of seconds, with an optional sign and fraction: `-1.5`, `.5`, `5.`, `+2`. */
const OFFSET = /^[+-]?(?:\d+(?:\.\d*)?|\.\d+)$/;

/** The offset as a number of seconds; 0 for empty or unreadable input (what `isOffsetValid` refuses). */
export function parseOffset(offset: string): number {
	const trimmed = offset.trim();
	return OFFSET.test(trimmed) ? Number(trimmed) : 0;
}

/** Whether the typed offset is a number (or empty, which means none). */
export function isOffsetValid(offset: string): boolean {
	const trimmed = offset.trim();
	return trimmed === "" || OFFSET.test(trimmed);
}

/** The output file name of the form for its first source. */
export function combineOutputName(form: Pick<CombineForm, "sources" | "container" | "template">): string {
	const first = form.sources[0];
	return first ? renderTemplate(form.template, first.name, form.container) : "";
}

/** The operation the preview and the submit both take. */
export function toCombineOperation(form: CombineForm): CombineOperation {
	return create(CombineOperationSchema, {
		sources: form.sources.map((source) => ({ input: source.id, offsetSeconds: parseOffset(source.offset) })),
		tracks: form.tracks.map((track) => ({
			source: track.source,
			streamIndex: track.streamIndex,
			language: track.language,
			title: track.title,
			default: track.default,
			forced: track.forced
		})),
		output: outputPath(form.folder, combineOutputName(form))
	});
}

/** Whether the stream of a source is already an output track. */
export function isAdded(tracks: readonly TrackDraft[], source: number, streamIndex: number): boolean {
	return tracks.some((track) => track.source === source && track.streamIndex === streamIndex);
}

/** `tracks` with the stream appended, unless it is in already. */
export function addTrack(tracks: readonly TrackDraft[], source: number, streamIndex: number): TrackDraft[] {
	return isAdded(tracks, source, streamIndex) ? [...tracks] : [...tracks, { source, streamIndex }];
}

/** `tracks` without the one at `position`. */
export function removeTrack(tracks: readonly TrackDraft[], position: number): TrackDraft[] {
	return tracks.filter((_, index) => index !== position);
}

/** `tracks` with the one at `position` changed. */
export function editTrack(
	tracks: readonly TrackDraft[],
	position: number,
	patch: Partial<Omit<TrackDraft, "source" | "streamIndex">>
): TrackDraft[] {
	return tracks.map((track, index) => (index === position ? { ...track, ...patch } : track));
}

/**
 * The sources after the selection changed: files that left take their tracks with them, files that
 * came are appended, and the sources that stay keep their order and offsets.
 */
export function syncSources(
	form: Pick<CombineForm, "sources" | "tracks">,
	files: readonly { id: string; name: string }[]
): Pick<CombineForm, "sources" | "tracks"> {
	const wanted = new Map(files.map((file) => [file.id, file]));
	const kept = form.sources.filter((source) => wanted.has(source.id));
	const keptIds = new Set(kept.map((source) => source.id));
	const sources = [...kept, ...files.filter((file) => !keptIds.has(file.id)).map((file) => ({ id: file.id, name: file.name, offset: "" }))];
	const positions = new Map(form.sources.map((source, index) => [index, sources.findIndex((s) => s.id === source.id)]));
	const tracks = form.tracks.flatMap((track) => {
		const source = positions.get(track.source) ?? -1;
		return source < 0 ? [] : [{ ...track, source }];
	});
	return { sources, tracks };
}

/** The form with the source at `position` changed. */
export function editSource(sources: readonly SourceDraft[], position: number, patch: Partial<Omit<SourceDraft, "id">>): SourceDraft[] {
	return sources.map((source, index) => (index === position ? { ...source, ...patch } : source));
}
