import type { Client } from "@connectrpc/connect";
import { type Ref, ref, watch } from "vue";

import { type MergeForm, toOperation } from "../lib/merge";
import { type AppError, toAppError } from "~/core/libs/errors";
import { useRpcClient } from "~/core/libs/rpc";
import { type CheckMergeResponse, MergeService } from "~/generated/muxmultitool/merge/v1/merge_pb";

/**
 * Runs `CheckMerge` whenever the form changes (two or more inputs), so the table, the problems and the
 * command always describe what a submit would run. A newer check wins over one still in flight.
 */
export function useMergeCheck(
	form: Readonly<Ref<MergeForm>>,
	client: Client<typeof MergeService> = useRpcClient(MergeService),
	delayMs = 200
) {
	const check = ref<CheckMergeResponse>();
	const checking = ref(false);
	const error = ref<AppError>();
	let latest = 0;
	let timer: ReturnType<typeof setTimeout> | undefined;

	async function run() {
		const call = ++latest;
		checking.value = true;
		try {
			const response = await client.checkMerge({ operation: toOperation(form.value) });
			if (call === latest) {
				check.value = response;
				error.value = undefined;
			}
		} catch (cause) {
			if (call === latest) {
				check.value = undefined;
				error.value = toAppError(cause);
			}
		} finally {
			if (call === latest) {
				checking.value = false;
			}
		}
	}

	watch(
		form,
		(value) => {
			clearTimeout(timer);
			if (value.inputs.length < 2) {
				latest++;
				check.value = undefined;
				error.value = undefined;
				checking.value = false;
				return;
			}
			checking.value = true;
			timer = setTimeout(() => void run(), delayMs);
		},
		{ deep: true, immediate: true }
	);

	return { check, checking, error, recheck: run };
}
