import type { Client } from "@connectrpc/connect";
import { computed, type Ref, shallowRef, watch } from "vue";

import {
	baseOperation,
	effectiveOperation,
	extractOperation,
	type NameFields,
	nameFields,
	needsEffectivePreview,
	overridesOf,
	type StreamsForm
} from "../lib/streams";
import { type AppError, toAppError } from "~/core/libs/errors";
import { useRpcClient } from "~/core/libs/rpc";
import { ExtractService, type PreviewExtractResponse } from "~/generated/muxmultitool/extract/v1/extract_pb";
import type { Entry } from "~/generated/muxmultitool/files/v1/files_pb";
import { type PreviewStreamsResponse, StreamsService } from "~/generated/muxmultitool/streams/v1/streams_pb";

/** What the backend says about one file of the selection. */
export interface FilePreview {
	/** Copy: the rules' resolution, which the table's rows and "why" column come from. */
	base?: PreviewStreamsResponse;
	/** Copy: what a submit runs; the base preview while nothing is ticked by hand. */
	effective?: PreviewStreamsResponse;
	/** Extract: the files the job would write and the streams it skips. */
	extract?: PreviewExtractResponse;
	error?: AppError;
	loading: boolean;
}

export interface PreviewClients {
	streams: Client<typeof StreamsService>;
	extract: Client<typeof ExtractService>;
}

/**
 * Runs the previews of every file of the selection whenever the form changes, so the table, the
 * issues and the command always describe what a submit would run. Only the files whose operation
 * changed are asked again; a newer preview of a file wins over one still in flight.
 */
export function useStreamPreviews(
	form: Readonly<Ref<StreamsForm>>,
	files: Readonly<Ref<readonly Entry[]>>,
	clients: PreviewClients = { streams: useRpcClient(StreamsService), extract: useRpcClient(ExtractService) },
	delayMs = 200
) {
	const previews = shallowRef(new Map<string, FilePreview>());
	const latest = new Map<string, number>();
	const keys = new Map<string, string>();
	let timer: ReturnType<typeof setTimeout> | undefined;

	function set(fileId: string, patch: Partial<FilePreview>) {
		const next = new Map(previews.value);
		next.set(fileId, { loading: false, ...next.get(fileId), ...patch });
		previews.value = next;
	}

	/**
	 * The output name's fields of a file: its place in the selection and, once a preview is in, the
	 * languages it keeps. A template with `{lang}` is previewed again when they become known (see
	 * `run`), which changes the name, never the streams, so it settles after one more round.
	 */
	function fieldsOf(file: Entry): NameFields {
		const preview = previews.value.get(file.id);
		const index = files.value.findIndex((entry) => entry.id === file.id);
		return nameFields(file, Math.max(index, 0), files.value.length, preview?.effective ?? preview?.base);
	}

	/** What the file's previews depend on; the same key means the same answer. */
	function keyOf(file: Entry): string {
		const current = form.value;
		const overrides = [...overridesOf(current.overrides, file.id)].toSorted((a, b) => a - b);
		const operation =
			current.action === "copy" ? baseOperation(current, file.id, file.name, fieldsOf(file)) : extractOperation(current, file.id);
		return JSON.stringify([current.action, operation, overrides]);
	}

	async function run(file: Entry) {
		const call = (latest.get(file.id) ?? 0) + 1;
		latest.set(file.id, call);
		const current = form.value;
		set(file.id, { loading: true });
		try {
			if (current.action === "extract") {
				const extract = await clients.extract.previewExtract({ operation: extractOperation(current, file.id) });
				if (latest.get(file.id) === call) {
					set(file.id, { extract, base: undefined, effective: undefined, error: undefined, loading: false });
				}
				return;
			}
			const fields = fieldsOf(file);
			const base = await clients.streams.previewStreams({ operation: baseOperation(current, file.id, file.name, fields) });
			if (latest.get(file.id) !== call) {
				return;
			}
			let effective = base;
			if (needsEffectivePreview(current, file.id, base)) {
				effective = await clients.streams.previewStreams({ operation: effectiveOperation(current, file.id, file.name, base, fields) });
				if (latest.get(file.id) !== call) {
					return;
				}
			}
			set(file.id, { base, effective, extract: undefined, error: undefined, loading: false });
			// The languages are known now; a name that uses them is previewed once more
			const settled = keyOf(file);
			if (keys.get(file.id) !== settled) {
				keys.set(file.id, settled);
				void run(file);
			}
		} catch (cause) {
			if (latest.get(file.id) === call) {
				set(file.id, { base: undefined, effective: undefined, extract: undefined, error: toAppError(cause), loading: false });
			}
		}
	}

	function refresh() {
		const wanted = new Set(files.value.map((file) => file.id));
		// Files that left the selection are forgotten
		for (const fileId of keys
			.keys()
			.filter((id) => !wanted.has(id))
			.toArray()) {
			keys.delete(fileId);
			latest.delete(fileId);
		}
		if ([...previews.value.keys()].some((fileId) => !wanted.has(fileId))) {
			previews.value = new Map([...previews.value].filter(([fileId]) => wanted.has(fileId)));
		}
		for (const file of files.value) {
			const key = keyOf(file);
			if (keys.get(file.id) !== key) {
				keys.set(file.id, key);
				void run(file);
			}
		}
	}

	watch(
		[form, files],
		() => {
			clearTimeout(timer);
			// Mark the changed files busy at once, so the screen never shows a stale result as current
			for (const file of files.value) {
				if (keys.get(file.id) !== keyOf(file)) {
					set(file.id, { loading: true });
				}
			}
			timer = setTimeout(refresh, delayMs);
		},
		{ deep: true, immediate: true }
	);

	const previewOf = (fileId: string): FilePreview | undefined => previews.value.get(fileId);
	/** The name fields a submit must use, so the job writes the name the preview showed. */
	const fieldsFor = (file: Entry): NameFields => fieldsOf(file);
	const loading = computed(() => [...previews.value.values()].some((preview) => preview.loading));

	/** Forgets what was previewed, so every file is asked again (after a submit, for instance). */
	function reset() {
		keys.clear();
		clearTimeout(timer);
		timer = setTimeout(refresh, 0);
	}

	return { previews, previewOf, fieldsFor, loading, reset };
}
