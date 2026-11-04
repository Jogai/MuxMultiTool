import type { Client } from "@connectrpc/connect";
import { computed, type Ref } from "vue";

import { type MergeForm, toOperation } from "../lib/merge";
import { useDebouncedCall } from "~/core/composables/useDebouncedCall";
import { useRpcClient } from "~/core/libs/rpc";
import { type CheckMergeRequest, MergeService } from "~/generated/muxmultitool/merge/v1/merge_pb";

/**
 * Runs `CheckMerge` whenever the form changes (two or more inputs), so the table, the problems and the
 * command always describe what a submit would run. A newer check wins over one still in flight.
 */
export function useMergeCheck(
	form: Readonly<Ref<MergeForm>>,
	client: Client<typeof MergeService> = useRpcClient(MergeService),
	delayMs = 200
) {
	const request = computed<Partial<CheckMergeRequest> | undefined>(() =>
		form.value.inputs.length < 2 ? undefined : { operation: toOperation(form.value) }
	);
	const { result, pending, error, refresh } = useDebouncedCall(request, (value, signal) => client.checkMerge(value, { signal }), delayMs);

	return { check: result, checking: pending, error, recheck: refresh };
}
