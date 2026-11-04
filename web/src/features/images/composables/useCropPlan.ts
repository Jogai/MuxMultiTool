import type { Client } from "@connectrpc/connect";
import { computed, type MaybeRefOrGetter } from "vue";

import { useDebouncedCall } from "~/core/composables/useDebouncedCall";
import { type CropValues, toCropOperation, toCropValues } from "../lib/crop";
import { useRpcClient } from "~/core/libs/rpc";
import { CropService } from "~/generated/muxmultitool/crop/v1/crop_pb";

export interface CropRequest {
	input: string;
	output: string;
	crop: CropValues;
}

/**
 * `PlanCrop` for the request as it changes: whether the file can be cropped (and why not), the
 * source stream with its crop unit, the snapped crop, the result size, the command and the
 * limitations. The support answer is what disables the Crop tab for other codecs.
 */
export function useCropPlan(
	request: MaybeRefOrGetter<CropRequest | undefined>,
	client: Client<typeof CropService> = useRpcClient(CropService),
	delayMs = 200
) {
	const { result, pending, error, refresh } = useDebouncedCall(
		request,
		(value, signal) => client.planCrop({ operation: toCropOperation(value.input, value.output, value.crop) }, { signal }),
		delayMs
	);
	const supported = computed(() => result.value?.support?.supported);
	const reason = computed(() => result.value?.support?.reason || undefined);
	const source = computed(() => result.value?.source);
	const snapped = computed(() => (result.value?.crop ? toCropValues(result.value.crop) : undefined));
	return { plan: result, planning: pending, error, supported, reason, source, snapped, refresh };
}
