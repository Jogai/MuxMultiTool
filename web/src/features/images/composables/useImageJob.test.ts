import { create } from "@bufbuild/protobuf";
import { Code, ConnectError } from "@connectrpc/connect";
import { describe, expect, it, vi } from "vitest";
import { shallowRef } from "vue";

import { useImageJob } from "./useImageJob";
import { type Job, JobSchema, JobState, SubmitJobResponseSchema } from "~/generated/muxmultitool/jobs/v1/jobs_pb";

type Client = Parameters<typeof useImageJob>[1];
const operation = { case: "snapshot", value: { input: "in", output: "out.png", positionMs: 1000n } } as const;

describe("useImageJob", () => {
	it("submits and follows the live job from the queue", async () => {
		const queued = create(JobSchema, { id: "j1", name: "out.png", state: JobState.QUEUED });
		const submitJob = vi.fn().mockResolvedValue(create(SubmitJobResponseSchema, { job: queued }));
		const live = shallowRef<Job>();
		const { job, submit, submitting, error, reset } = useImageJob((id) => (live.value?.id === id ? live.value : undefined), {
			submitJob
		} as unknown as Client);

		const promise = submit(operation, "a name");
		expect(submitting.value).toBe(true);
		expect(await promise).toBe(queued);
		expect(submitJob).toHaveBeenCalledWith({ name: "a name", operation });
		expect(job.value?.state).toBe(JobState.QUEUED);

		live.value = create(JobSchema, { id: "j1", state: JobState.SUCCEEDED });
		expect(job.value?.state).toBe(JobState.SUCCEEDED);
		expect(error.value).toBeUndefined();

		reset();
		expect(job.value).toBeUndefined();
	});

	it("keeps the refusal", async () => {
		const submitJob = vi.fn().mockRejectedValue(new ConnectError("over the limits", Code.FailedPrecondition));
		const { job, submit, error } = useImageJob(() => undefined, { submitJob } as unknown as Client);
		expect(await submit(operation)).toBeUndefined();
		expect(error.value?.message).toBe("over the limits");
		expect(job.value).toBeUndefined();
	});
});
