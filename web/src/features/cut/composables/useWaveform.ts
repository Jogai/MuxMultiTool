import type { Client } from "@connectrpc/connect";
import { type MaybeRefOrGetter, onScopeDispose, ref, shallowRef, toValue, watch } from "vue";

import { type AppError, toAppError } from "~/core/libs/errors";
import { useRpcClient } from "~/core/libs/rpc";
import { MediaService } from "~/generated/muxmultitool/media/v1/media_pb";

/** The peaks of one window, as the lane draws them. */
export interface WaveformPeaks {
	from: number;
	to: number;
	count: number;
	mins: Float32Array;
	maxs: Float32Array;
	/** How many peaks from the start are filled in. */
	received: number;
	/** How far the background analysis of the stream got. */
	analyzedSeconds: number;
	done: boolean;
}

/** `GetWaveform` returns at most this many peaks per window. */
export const MAX_PEAKS = 8192;

export interface WaveformRequest {
	from: number;
	to: number;
	/** The resolution: one peak per pixel of the lane. */
	peakCount: number;
}

/**
 * The waveform peaks of one audio stream for the visible window, re-requested (after a short pause) when
 * the window or the stream changes and filled in as the server analyses the stream. One open stream at a
 * time; a new window drops the old one.
 */
export function useWaveform(
	fileId: MaybeRefOrGetter<string | undefined>,
	streamIndex: MaybeRefOrGetter<number | undefined>,
	request: MaybeRefOrGetter<WaveformRequest | undefined>,
	client: Client<typeof MediaService> = useRpcClient(MediaService),
	delayMs = 120
) {
	const peaks = shallowRef<WaveformPeaks>();
	const loading = ref(false);
	const error = ref<AppError>();
	let controller: AbortController | undefined;
	let timer: ReturnType<typeof setTimeout> | undefined;

	function stop(): void {
		controller?.abort();
		controller = undefined;
		loading.value = false;
	}

	async function load(): Promise<void> {
		stop();
		const id = toValue(fileId);
		const index = toValue(streamIndex);
		const window = toValue(request);
		if (!id || index === undefined || !window || !(window.to > window.from) || !(window.peakCount > 0)) {
			peaks.value = undefined;
			return;
		}
		const mine = new AbortController();
		controller = mine;
		loading.value = true;
		error.value = undefined;
		const peakCount = Math.min(MAX_PEAKS, Math.round(window.peakCount));
		let current: WaveformPeaks | undefined;
		try {
			const stream = client.getWaveform(
				{ fileId: id, streamIndex: index, fromSeconds: window.from, toSeconds: window.to, peakCount },
				{ signal: mine.signal }
			);
			for await (const message of stream) {
				if (mine.signal.aborted) {
					return;
				}
				if (!current || current.count !== message.peakCount) {
					current = {
						from: message.fromSeconds,
						to: message.toSeconds,
						count: message.peakCount,
						mins: new Float32Array(message.peakCount),
						maxs: new Float32Array(message.peakCount),
						received: 0,
						analyzedSeconds: 0,
						done: false
					};
				}
				current.mins.set(message.mins.slice(0, current.count - message.offset), message.offset);
				current.maxs.set(message.maxs.slice(0, current.count - message.offset), message.offset);
				current.received = Math.max(current.received, Math.min(current.count, message.offset + message.mins.length));
				current.analyzedSeconds = message.analyzedSeconds;
				current.done = message.done;
				// A new object each time, so a shallow ref sees the change
				peaks.value = { ...current };
			}
		} catch (cause) {
			if (!mine.signal.aborted) {
				error.value = toAppError(cause);
			}
		} finally {
			if (controller === mine) {
				loading.value = false;
			}
		}
	}

	watch(
		[() => toValue(fileId), () => toValue(streamIndex)],
		() => {
			peaks.value = undefined;
			clearTimeout(timer);
			void load();
		},
		{ immediate: true }
	);
	watch(
		() => toValue(request),
		() => {
			clearTimeout(timer);
			timer = setTimeout(() => void load(), delayMs);
		}
	);
	onScopeDispose(() => {
		clearTimeout(timer);
		stop();
	});

	return { peaks, loading, error };
}
