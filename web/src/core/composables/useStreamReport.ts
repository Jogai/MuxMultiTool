import type { Client } from "@connectrpc/connect";
import { computed, ref, shallowRef } from "vue";

import { type AppError, toAppError } from "~/core/libs/errors";
import { useRpcClient } from "~/core/libs/rpc";
import { type FileReport, FilesService, type GetStreamDetailsResponse } from "~/generated/muxmultitool/files/v1/files_pb";

/** How a report was made: quick (sizes from tags or estimates) or measured (the packet pass ran). */
export interface CachedReport {
	report: FileReport;
	measured: boolean;
}

/**
 * The stream report of the selected files, one `FileReport` per file id, cached so that changing the
 * selection fetches only the files not reported yet. A quick report (no packet pass) is the default,
 * because the pass reads every byte of a file; `measure()` reruns the selection exactly.
 */
export function useStreamReport(client: Client<typeof FilesService> = useRpcClient(FilesService)) {
	const cache = shallowRef(new Map<string, CachedReport>());
	const loading = ref(false);
	const error = ref<AppError>();
	let latest = 0;

	/** The cached report of a file, or `undefined` while it loads. */
	function byId(id: string): CachedReport | undefined {
		return cache.value.get(id);
	}

	/** The reports of `ids` in that order, leaving out the files not loaded yet. */
	function reportsFor(ids: readonly string[]): FileReport[] {
		return ids.flatMap((id) => {
			const cached = cache.value.get(id);
			return cached ? [cached.report] : [];
		});
	}

	const measuredAll = computed(() => [...cache.value.values()].every((cached) => cached.measured));

	/**
	 * Fetches the reports of `ids` that are not cached yet; with `measure`, every id whose report is not
	 * measured yet. A newer call wins over an older one still in flight, but both store what arrives.
	 */
	async function load(ids: readonly string[], { measure = false }: { measure?: boolean } = {}): Promise<void> {
		const wanted = ids.filter((id) => {
			const cached = cache.value.get(id);
			return !cached || (measure && !cached.measured);
		});
		if (wanted.length === 0) {
			return;
		}
		const call = ++latest;
		loading.value = true;
		error.value = undefined;
		try {
			const response = await client.getStreamReport({ fileIds: wanted, quick: !measure });
			// A new map, so a component that received the old one as a prop sees the change
			const next = new Map(cache.value);
			for (const report of response.files) {
				next.set(report.fileId, { report, measured: measure });
			}
			cache.value = next;
		} catch (cause) {
			if (call === latest) {
				error.value = toAppError(cause);
			}
		} finally {
			if (call === latest) {
				loading.value = false;
			}
		}
	}

	/** Reruns the reports of `ids` with the packet pass, for exact sizes. */
	function measure(ids: readonly string[]): Promise<void> {
		return load(ids, { measure: true });
	}

	/** Drops the cached reports of `ids`, so the next `load` probes them again. */
	function forget(ids: readonly string[]): void {
		const next = new Map(cache.value);
		for (const id of ids) {
			next.delete(id);
		}
		cache.value = next;
	}

	const detailsCache = new Map<string, Promise<GetStreamDetailsResponse>>();

	/** Everything ffprobe reports about one stream, fetched once per stream. Rejects with an `AppError`. */
	function details(fileId: string, streamIndex: number): Promise<GetStreamDetailsResponse> {
		const key = `${fileId}:${streamIndex}`;
		let pending = detailsCache.get(key);
		if (!pending) {
			pending = client.getStreamDetails({ fileId, streamIndex }).catch((cause: unknown) => {
				detailsCache.delete(key);
				throw toAppError(cause);
			});
			detailsCache.set(key, pending);
		}
		return pending;
	}

	return { cache, loading, error, measuredAll, byId, reportsFor, load, measure, forget, details };
}
