import type { Client } from "@connectrpc/connect";
import { type MaybeRefOrGetter, onScopeDispose, ref, shallowRef, toValue, watch } from "vue";

import { type AppError, toAppError } from "~/core/libs/errors";
import { useRpcClient } from "~/core/libs/rpc";
import { MediaService, type ThumbnailSheet } from "~/generated/muxmultitool/media/v1/media_pb";

/**
 * The thumbnail sheet of the file, streamed from the media service as the sprites are generated: the
 * layout first, then the count of ready sprites until the analysis is done. The lane draws what is ready
 * and fills in as more arrives.
 */
export function useThumbnails(
	fileId: MaybeRefOrGetter<string | undefined>,
	client: Client<typeof MediaService> = useRpcClient(MediaService)
) {
	const sheet = shallowRef<ThumbnailSheet>();
	const spritesReady = ref(0);
	const done = ref(false);
	const error = ref<AppError>();
	let controller: AbortController | undefined;

	async function load(): Promise<void> {
		controller?.abort();
		controller = undefined;
		sheet.value = undefined;
		spritesReady.value = 0;
		done.value = false;
		error.value = undefined;
		const id = toValue(fileId);
		if (!id) {
			return;
		}
		const mine = new AbortController();
		controller = mine;
		try {
			for await (const message of client.getThumbnails({ fileId: id }, { signal: mine.signal })) {
				if (mine.signal.aborted) {
					return;
				}
				sheet.value = message.sheet;
				spritesReady.value = message.spritesReady;
				done.value = message.done;
			}
		} catch (cause) {
			if (!mine.signal.aborted) {
				error.value = toAppError(cause);
			}
		}
	}

	watch(
		() => toValue(fileId),
		() => void load(),
		{ immediate: true }
	);
	onScopeDispose(() => controller?.abort());

	return { sheet, spritesReady, done, error };
}
