import type { Client } from "@connectrpc/connect";
import { type MaybeRefOrGetter, onScopeDispose, ref, shallowRef, toValue, watch } from "vue";

import { type AppError, toAppError } from "~/core/libs/errors";
import { useRpcClient } from "~/core/libs/rpc";
import { type CheckSmartCutResponse, CutService } from "~/generated/muxmultitool/cut/v1/cut_pb";

/**
 * Whether the file can be smart cut (`CheckSmartCut`): H.264 or H.265, progressive, with the encoder in
 * the ffmpeg build. The cut mode choice shows the reason when it cannot.
 */
export function useSmartCut(fileId: MaybeRefOrGetter<string | undefined>, client: Client<typeof CutService> = useRpcClient(CutService)) {
	const check = shallowRef<CheckSmartCutResponse>();
	const checking = ref(false);
	const error = ref<AppError>();
	let generation = 0;

	async function load(): Promise<void> {
		generation += 1;
		const mine = generation;
		check.value = undefined;
		error.value = undefined;
		const id = toValue(fileId);
		if (!id) {
			checking.value = false;
			return;
		}
		checking.value = true;
		try {
			const response = await client.checkSmartCut({ fileId: id });
			if (mine === generation) {
				check.value = response;
			}
		} catch (cause) {
			if (mine === generation) {
				error.value = toAppError(cause);
			}
		} finally {
			if (mine === generation) {
				checking.value = false;
			}
		}
	}

	watch(
		() => toValue(fileId),
		() => void load(),
		{ immediate: true }
	);
	onScopeDispose(() => {
		generation += 1;
	});

	return { check, checking, error };
}
