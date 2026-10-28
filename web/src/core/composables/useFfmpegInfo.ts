import type { Client } from "@connectrpc/connect";
import { ref } from "vue";

import { type AppError, toAppError } from "~/core/libs/errors";
import { useRpcClient } from "~/core/libs/rpc";
import { CapabilitiesStatus, SystemService } from "~/generated/muxmultitool/system/v1/system_pb";

/** The installed ffmpeg as the capability check reports it. */
export function useFfmpegInfo(client: Client<typeof SystemService> = useRpcClient(SystemService)) {
	const state = ref<"idle" | "loading" | "ok" | "unavailable" | "error">("idle");
	const version = ref<string>();
	const reason = ref<string>();
	const error = ref<AppError>();

	async function load() {
		state.value = "loading";
		error.value = undefined;
		try {
			const response = await client.getCapabilities({});
			if (response.status === CapabilitiesStatus.AVAILABLE) {
				version.value = response.capabilities?.version;
				state.value = "ok";
			} else {
				reason.value = response.error || undefined;
				state.value = "unavailable";
			}
		} catch (cause) {
			error.value = toAppError(cause);
			state.value = "error";
		}
	}

	return { state, version, reason, error, load };
}
