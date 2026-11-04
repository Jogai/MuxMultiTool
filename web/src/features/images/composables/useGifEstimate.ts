import type { Client } from "@connectrpc/connect";
import type { MaybeRefOrGetter } from "vue";

import { useDebouncedCall } from "./useDebouncedCall";
import { type GifForm, toGifOperation } from "../lib/gif";
import { useRpcClient } from "~/core/libs/rpc";
import { GifService } from "~/generated/muxmultitool/gif/v1/gif_pb";

/** `EstimateGif` for the form as it changes: the output size, frame count, size estimate, violations and command. */
export function useGifEstimate(
	form: MaybeRefOrGetter<GifForm | undefined>,
	client: Client<typeof GifService> = useRpcClient(GifService),
	delayMs = 200
) {
	const { result, pending, error, refresh } = useDebouncedCall(
		form,
		(value, signal) => client.estimateGif({ operation: toGifOperation(value) }, { signal }),
		delayMs
	);
	return { estimate: result, estimating: pending, error, refresh };
}
