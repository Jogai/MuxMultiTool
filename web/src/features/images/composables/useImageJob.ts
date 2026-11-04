import type { MessageInitShape } from "@bufbuild/protobuf";
import type { Client } from "@connectrpc/connect";
import { computed, ref, shallowRef } from "vue";

import { type AppError, toAppError } from "~/core/libs/errors";
import { useRpcClient } from "~/core/libs/rpc";
import { type Job, JobsService, type SubmitJobRequestSchema } from "~/generated/muxmultitool/jobs/v1/jobs_pb";

/** One operation of the submit oneof, as a plain init object. */
export type ImageOperation = NonNullable<MessageInitShape<typeof SubmitJobRequestSchema>["operation"]>;

/**
 * Submits one job from an image panel and follows it: the job as the submit returned it, replaced by
 * the live one from the queue once the queue stream knows it. `lookup` is the queue's `byId`; the
 * panels pass the shared jobs store so a finished job shows its download without the Jobs screen.
 */
export function useImageJob(lookup: (id: string) => Job | undefined, client: Client<typeof JobsService> = useRpcClient(JobsService)) {
	const submitted = shallowRef<Job>();
	const submitting = ref(false);
	const error = ref<AppError>();

	const job = computed(() => (submitted.value ? (lookup(submitted.value.id) ?? submitted.value) : undefined));

	/** Submits `operation`; resolves with the job, or with nothing when the server refused (see `error`). */
	async function submit(operation: ImageOperation, name = ""): Promise<Job | undefined> {
		error.value = undefined;
		submitted.value = undefined;
		submitting.value = true;
		try {
			const response = await client.submitJob({ name, operation });
			submitted.value = response.job;
			return response.job;
		} catch (cause) {
			error.value = toAppError(cause);
			return undefined;
		} finally {
			submitting.value = false;
		}
	}

	function reset(): void {
		submitted.value = undefined;
		error.value = undefined;
	}

	return { job, submitting, error, submit, reset };
}
