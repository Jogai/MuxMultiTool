import type { Client } from "@connectrpc/connect";
import { computed, ref, shallowRef } from "vue";

import { type ImageSupport, supportOf } from "../lib/support";
import { type AppError, toAppError } from "~/core/libs/errors";
import { useRpcClient } from "~/core/libs/rpc";
import { type GetCapabilitiesResponse, SystemService } from "~/generated/muxmultitool/system/v1/system_pb";

/**
 * The capability check, read once for the screen: which image options the ffmpeg build supports.
 * Until it answers every option is enabled; a build without ffmpeg disables them all.
 */
export function useImageSupport(client: Client<typeof SystemService> = useRpcClient(SystemService)) {
	const response = shallowRef<GetCapabilitiesResponse>();
	const error = ref<AppError>();
	const support = computed<ImageSupport>(() => supportOf(response.value));
	const unavailable = computed(() => (response.value?.error ?? "") || undefined);

	async function load(): Promise<void> {
		try {
			response.value = await client.getCapabilities({});
			error.value = undefined;
		} catch (cause) {
			error.value = toAppError(cause);
		}
	}
	void load();

	return { support, unavailable, error, reload: load };
}
