import type { Client } from "@connectrpc/connect";
import { computed, ref, shallowRef } from "vue";

import { type AppError, toAppError } from "~/core/libs/errors";
import { useRpcClient } from "~/core/libs/rpc";
import { type Preset, PresetsService } from "~/generated/muxmultitool/presets/v1/presets_pb";

/**
 * The saved presets as the server lists them, for the Presets screen and the Batch screen's preset
 * choice. Dialogs that create, change or delete a preset report the result here (`replace`, `remove`)
 * so the list is right at once, without another round trip.
 */
export function usePresets(client: Client<typeof PresetsService> = useRpcClient(PresetsService)) {
	const presets = shallowRef<readonly Preset[]>([]);
	const loading = ref(false);
	const loaded = ref(false);
	const error = ref<AppError>();

	async function load(): Promise<void> {
		loading.value = true;
		try {
			presets.value = (await client.listPresets({})).presets;
			error.value = undefined;
			loaded.value = true;
		} catch (cause) {
			error.value = toAppError(cause);
		} finally {
			loading.value = false;
		}
	}

	const byId = computed(() => new Map(presets.value.map((preset) => [preset.id, preset])));

	/** Puts a created or changed preset into the list, in the server's order (creation order). */
	function replace(preset: Preset): void {
		const index = presets.value.findIndex((known) => known.id === preset.id);
		presets.value = index === -1 ? [...presets.value, preset] : presets.value.with(index, preset);
	}

	function remove(id: string): void {
		presets.value = presets.value.filter((preset) => preset.id !== id);
	}

	return { presets, loading, loaded, error, load, byId, replace, remove };
}
