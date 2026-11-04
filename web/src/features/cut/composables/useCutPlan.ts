import type { Client } from "@connectrpc/connect";
import { computed, type MaybeRefOrGetter, toValue } from "vue";

import { useDebouncedCall } from "~/core/composables/useDebouncedCall";
import { useRpcClient } from "~/core/libs/rpc";
import { CutService, type PlanCutRequest } from "~/generated/muxmultitool/cut/v1/cut_pb";

import { canExport, toCutOperation } from "../lib/export";
import type { EditorState } from "../lib/segments";

/**
 * Runs `PlanCut` whenever the segments, the modes or the output change (after a short pause), so the
 * panel always shows where the cuts land, the outputs and the command a submit would run, and the
 * reason when the plan is refused (a smart cut of an unsupported file, an existing output, ...).
 */
export function useCutPlan(
	fileId: MaybeRefOrGetter<string | undefined>,
	state: MaybeRefOrGetter<EditorState>,
	output: MaybeRefOrGetter<string>,
	client: Client<typeof CutService> = useRpcClient(CutService),
	delayMs = 250
) {
	const request = computed<Partial<PlanCutRequest> | undefined>(() => {
		const id = toValue(fileId);
		const current = toValue(state);
		const target = toValue(output);
		return id && canExport(current, target) ? { operation: toCutOperation(id, current, target) } : undefined;
	});
	const { result, pending, error, refresh } = useDebouncedCall(request, (value, signal) => client.planCut(value, { signal }), delayMs);

	return { plan: result, planning: pending, error, replan: refresh };
}
