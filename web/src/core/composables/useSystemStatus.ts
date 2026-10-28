import type { Client } from "@connectrpc/connect";
import { ref } from "vue";

import { type AppError, toAppError } from "~/core/libs/errors";
import { useRpcClient } from "~/core/libs/rpc";
import { HealthStatus, SystemService } from "~/generated/muxmultitool/system/v1/system_pb";

/** Whether the backend answered yet, and how. */
export type BackendState = "loading" | "ok" | "error";

/**
 * Asks the backend for its health and version, right away and on `refresh()`.
 * The status bar shows the version; the settings page shows the error when the backend is unreachable.
 */
export function useSystemStatus(client: Client<typeof SystemService> = useRpcClient(SystemService)) {
	const state = ref<BackendState>("loading");
	const version = ref<string>();
	const error = ref<AppError>();

	async function refresh() {
		state.value = "loading";
		error.value = undefined;
		try {
			const [health, info] = await Promise.all([client.getHealth({}), client.getVersion({})]);
			version.value = info.version;
			state.value = health.status === HealthStatus.OK ? "ok" : "error";
		} catch (cause) {
			state.value = "error";
			error.value = toAppError(cause);
		}
	}

	void refresh();

	return { state, version, error, refresh };
}
