import { type MaybeRefOrGetter, onScopeDispose, ref, shallowRef, toValue, watch } from "vue";

import { type AppError, toAppError } from "~/core/libs/errors";
import { useRpcClient } from "~/core/libs/rpc";
import { MediaService, type StartPreviewResponse } from "~/generated/muxmultitool/media/v1/media_pb";

export type PreviewStatus = "idle" | "starting" | "ready" | "error";

/**
 * The HLS preview session of a file: `StartPreview` when the id is set or changes, `StopPreview` for the
 * previous session and when the scope ends. The playlist is then fetched by URL from the video element.
 */
export function usePreviewSession(fileId: MaybeRefOrGetter<string | undefined>) {
	const client = useRpcClient(MediaService);
	const session = shallowRef<StartPreviewResponse>();
	const status = ref<PreviewStatus>("idle");
	const error = ref<AppError>();
	let generation = 0;

	function stopCurrent(): void {
		const current = session.value;
		session.value = undefined;
		if (current) {
			// Fire and forget: the server also expires idle sessions on its own
			client.stopPreview({ mediaId: current.mediaId }).catch(() => {});
		}
	}

	async function start(): Promise<void> {
		generation += 1;
		const mine = generation;
		stopCurrent();
		error.value = undefined;
		const id = toValue(fileId);
		if (!id) {
			status.value = "idle";
			return;
		}
		status.value = "starting";
		try {
			const response = await client.startPreview({ fileId: id });
			if (mine !== generation) {
				// The file changed while this call ran; its session is not ours any more
				client.stopPreview({ mediaId: response.mediaId }).catch(() => {});
				return;
			}
			session.value = response;
			status.value = "ready";
		} catch (cause) {
			if (mine === generation) {
				error.value = toAppError(cause);
				status.value = "error";
			}
		}
	}

	watch(
		() => toValue(fileId),
		() => void start(),
		{ immediate: true }
	);
	onScopeDispose(() => {
		generation += 1;
		stopCurrent();
	});

	return { session, status, error, restart: start };
}
