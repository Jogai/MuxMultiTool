import type { Client } from "@connectrpc/connect";
import { computed, type MaybeRefOrGetter, onScopeDispose, ref, shallowRef, toValue, watch } from "vue";

import { type AppError, toAppError } from "~/core/libs/errors";
import { useRpcClient } from "~/core/libs/rpc";
import { CutService } from "~/generated/muxmultitool/cut/v1/cut_pb";

import { emptyKeyframeCache, type KeyframeCache, keyframeWindow, mergeKeyframes, missingRanges } from "../lib/keyframes";

/** A time range of the file. */
export interface TimeWindow {
	from: number;
	to: number;
}

/**
 * The longest visible range whose keyframes are fetched: zoomed out further than this on a long file,
 * the lane stays empty rather than reading the whole file.
 */
export const KEYFRAME_MAX_VISIBLE_SECONDS = 1200;

/** How far around the playhead keyframes are always known, for stepping and snapping near it. */
export const KEYFRAME_PLAYHEAD_MARGIN = 120;

/**
 * The keyframes of the file around what the timeline shows and around the playhead, fetched in blocks
 * as the view moves (`GetKeyframes` reads the file between two times, so never the whole of a long one)
 * and merged into one ascending list for the lane, the snapping and the player's keyframe stepping.
 */
export function useKeyframes(
	fileId: MaybeRefOrGetter<string | undefined>,
	visible: MaybeRefOrGetter<TimeWindow | undefined>,
	duration: MaybeRefOrGetter<number>,
	playhead: MaybeRefOrGetter<number>,
	client: Client<typeof CutService> = useRpcClient(CutService),
	delayMs = 150
) {
	const cache = shallowRef<KeyframeCache>(emptyKeyframeCache());
	/** The file's length as the keyframe responses report it; 0 until the first one. */
	const known = ref(0);
	const loading = ref(false);
	const error = ref<AppError>();
	const times = computed(() => cache.value.times);
	const inFlight = new Set<string>();
	let generation = 0;
	let timer: ReturnType<typeof setTimeout> | undefined;

	function wanted(): Array<[number, number]> {
		const length = toValue(duration) || known.value;
		if (!(length > 0)) {
			// Nothing known about the file yet: the start of it, which also tells its length
			return [[0, KEYFRAME_PLAYHEAD_MARGIN]];
		}
		const windows: Array<[number, number]> = [];
		const view = toValue(visible);
		if (view && (view.to - view.from <= KEYFRAME_MAX_VISIBLE_SECONDS || length <= KEYFRAME_MAX_VISIBLE_SECONDS)) {
			windows.push(keyframeWindow(view.from, view.to, length));
		}
		const at = toValue(playhead);
		windows.push(keyframeWindow(Math.max(0, at - KEYFRAME_PLAYHEAD_MARGIN), Math.min(length, at + KEYFRAME_PLAYHEAD_MARGIN), length));
		return windows;
	}

	async function fetchRange(id: string, from: number, to: number, mine: number): Promise<void> {
		const key = `${from}-${to}`;
		if (inFlight.has(key)) {
			return;
		}
		inFlight.add(key);
		loading.value = true;
		try {
			const response = await client.getKeyframes({ fileId: id, fromSeconds: from, toSeconds: to });
			if (mine === generation) {
				cache.value = mergeKeyframes(cache.value, from, to, response.keyframesSeconds);
				if (response.durationSeconds > 0) {
					known.value = response.durationSeconds;
				}
				error.value = undefined;
			}
		} catch (cause) {
			if (mine === generation) {
				error.value = toAppError(cause);
			}
		} finally {
			inFlight.delete(key);
			if (inFlight.size === 0) {
				loading.value = false;
			}
		}
	}

	function fetchMissing(): void {
		const id = toValue(fileId);
		if (!id) {
			return;
		}
		const mine = generation;
		for (const [from, to] of wanted()) {
			for (const [start, end] of missingRanges(cache.value, from, to)) {
				if (end > start) {
					void fetchRange(id, start, end, mine);
				}
			}
		}
	}

	function schedule(): void {
		clearTimeout(timer);
		timer = setTimeout(fetchMissing, delayMs);
	}

	watch(
		() => toValue(fileId),
		() => {
			generation += 1;
			cache.value = emptyKeyframeCache();
			known.value = 0;
			error.value = undefined;
			inFlight.clear();
			loading.value = false;
			schedule();
		},
		{ immediate: true }
	);
	watch([() => toValue(visible), () => toValue(duration), () => Math.round(toValue(playhead) / 30)], schedule);
	onScopeDispose(() => {
		generation += 1;
		clearTimeout(timer);
	});

	return { times, cache, duration: known, loading, error, refresh: fetchMissing };
}
