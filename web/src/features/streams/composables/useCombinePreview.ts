import type { Client } from "@connectrpc/connect";
import { computed, type Ref } from "vue";

import { type CombineForm, isOffsetValid, toCombineOperation } from "../lib/combine";
import { useDebouncedCall } from "~/core/composables/useDebouncedCall";
import { useRpcClient } from "~/core/libs/rpc";
import { CombineService, type PreviewCombineRequest } from "~/generated/muxmultitool/combine/v1/combine_pb";

/**
 * Runs `PreviewCombine` whenever the form changes (at least one source and one track), so the output
 * tracks, the warnings and the command always describe what a submit would run. A newer preview wins
 * over one still in flight.
 */
export function useCombinePreview(
	form: Readonly<Ref<CombineForm>>,
	client: Client<typeof CombineService> = useRpcClient(CombineService),
	delayMs = 200
) {
	const request = computed<Partial<PreviewCombineRequest> | undefined>(() => {
		const value = form.value;
		if (value.sources.length === 0 || value.tracks.length === 0 || !value.sources.every((source) => isOffsetValid(source.offset))) {
			return undefined;
		}
		return { operation: toCombineOperation(value) };
	});
	const { result, pending, error, refresh } = useDebouncedCall(
		request,
		(value, signal) => client.previewCombine(value, { signal }),
		delayMs
	);

	return { preview: result, checking: pending, error, refresh };
}
