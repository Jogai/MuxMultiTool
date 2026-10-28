import type { Client } from "@connectrpc/connect";
import { ref } from "vue";

import { createSystemClient } from "~/core/libs/rpc";
import { HealthStatus, type SystemService } from "~/generated/muxmultitool/system/v1/system_pb";

/** Whether the backend answered yet, and how. */
export type BackendState = "loading" | "ok" | "error";

/**
 * Asks the backend for its health and version, right away and on `refresh()`.
 * The first screen uses it to prove the RPC path works; the status bar will show the version.
 */
export function useSystemStatus(client: Client<typeof SystemService> = createSystemClient()) {
	const state = ref<BackendState>("loading");
	const version = ref<string>();
	const error = ref<string>();

	async function refresh() {
		state.value = "loading";
		error.value = undefined;
		try {
			const [health, info] = await Promise.all([client.getHealth({}), client.getVersion({})]);
			version.value = info.version;
			state.value = health.status === HealthStatus.OK ? "ok" : "error";
		} catch (cause) {
			state.value = "error";
			error.value = cause instanceof Error ? cause.message : String(cause);
		}
	}

	void refresh();

	return { state, version, error, refresh };
}
