import { type MaybeRefOrGetter, onScopeDispose, ref, shallowRef, toValue, watch } from "vue";

import { type AppError, toAppError } from "~/core/libs/errors";
import { useRpcClient } from "~/core/libs/rpc";
import { MediaService, type SubtitleCue } from "~/generated/muxmultitool/media/v1/media_pb";

import { sortCues } from "../lib/cues";

export type CuesStatus = "idle" | "loading" | "done" | "error";

/**
 * The cues of the selected subtitle stream, streamed from the media service as they are read and kept
 * sorted by start time. Changing the file or the stream drops the old cues and the open stream.
 */
export function useSubtitleCues(fileId: MaybeRefOrGetter<string | undefined>, streamIndex: MaybeRefOrGetter<number | undefined>) {
	const client = useRpcClient(MediaService);
	const cues = shallowRef<readonly SubtitleCue[]>([]);
	const status = ref<CuesStatus>("idle");
	const error = ref<AppError>();
	let controller: AbortController | undefined;

	async function load(): Promise<void> {
		controller?.abort();
		controller = undefined;
		cues.value = [];
		error.value = undefined;
		const id = toValue(fileId);
		const index = toValue(streamIndex);
		if (!id || index === undefined) {
			status.value = "idle";
			return;
		}
		const mine = new AbortController();
		controller = mine;
		status.value = "loading";
		const received: SubtitleCue[] = [];
		try {
			for await (const message of client.getSubtitleCues({ fileId: id, streamIndex: index }, { signal: mine.signal })) {
				if (mine.signal.aborted) {
					return;
				}
				received.push(...message.cues);
				cues.value = sortCues(received);
			}
			if (!mine.signal.aborted) {
				status.value = "done";
			}
		} catch (cause) {
			if (!mine.signal.aborted) {
				error.value = toAppError(cause);
				status.value = "error";
			}
		}
	}

	watch([() => toValue(fileId), () => toValue(streamIndex)], () => void load(), { immediate: true });
	onScopeDispose(() => controller?.abort());

	return { cues, status, error };
}
