import type { Client } from "@connectrpc/connect";
import { type Ref, ref, watch } from "vue";

import { type CombineForm, isOffsetValid, toCombineOperation } from "../lib/combine";
import { type AppError, toAppError } from "~/core/libs/errors";
import { useRpcClient } from "~/core/libs/rpc";
import { CombineService, type PreviewCombineResponse } from "~/generated/muxmultitool/combine/v1/combine_pb";

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
	const preview = ref<PreviewCombineResponse>();
	const checking = ref(false);
	const error = ref<AppError>();
	let latest = 0;
	let timer: ReturnType<typeof setTimeout> | undefined;

	async function run() {
		const call = ++latest;
		checking.value = true;
		try {
			const response = await client.previewCombine({ operation: toCombineOperation(form.value) });
			if (call === latest) {
				preview.value = response;
				error.value = undefined;
			}
		} catch (cause) {
			if (call === latest) {
				preview.value = undefined;
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
			if (value.sources.length === 0 || value.tracks.length === 0 || !value.sources.every((source) => isOffsetValid(source.offset))) {
				latest++;
				preview.value = undefined;
				error.value = undefined;
				checking.value = false;
				return;
			}
			checking.value = true;
			timer = setTimeout(() => void run(), delayMs);
		},
		{ deep: true, immediate: true }
	);

	return { preview, checking, error, refresh: run };
}
