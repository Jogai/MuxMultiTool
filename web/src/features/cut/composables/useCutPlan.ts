import type { Client } from "@connectrpc/connect";
import { type MaybeRefOrGetter, onScopeDispose, ref, shallowRef, toValue, watch } from "vue";

import { type AppError, toAppError } from "~/core/libs/errors";
import { useRpcClient } from "~/core/libs/rpc";
import { CutService, type PlanCutResponse } from "~/generated/muxmultitool/cut/v1/cut_pb";

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
	const plan = shallowRef<PlanCutResponse>();
	const planning = ref(false);
	const error = ref<AppError>();
	let latest = 0;
	let timer: ReturnType<typeof setTimeout> | undefined;

	async function run(): Promise<void> {
		const call = ++latest;
		const id = toValue(fileId);
		const current = toValue(state);
		const target = toValue(output);
		if (!id || !canExport(current, target)) {
			plan.value = undefined;
			error.value = undefined;
			planning.value = false;
			return;
		}
		planning.value = true;
		try {
			const response = await client.planCut({ operation: toCutOperation(id, current, target) });
			if (call === latest) {
				plan.value = response;
				error.value = undefined;
			}
		} catch (cause) {
			if (call === latest) {
				plan.value = undefined;
				error.value = toAppError(cause);
			}
		} finally {
			if (call === latest) {
				planning.value = false;
			}
		}
	}

	watch(
		[() => toValue(fileId), () => toValue(state), () => toValue(output)],
		() => {
			clearTimeout(timer);
			latest += 1;
			planning.value = canExport(toValue(state), toValue(output));
			timer = setTimeout(() => void run(), delayMs);
		},
		{ deep: true, immediate: true }
	);
	onScopeDispose(() => clearTimeout(timer));

	return { plan, planning, error, replan: run };
}
