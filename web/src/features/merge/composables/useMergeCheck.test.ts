import { create } from "@bufbuild/protobuf";
import { Code, ConnectError } from "@connectrpc/connect";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { nextTick, ref } from "vue";

import { useMergeCheck } from "./useMergeCheck";
import type { MergeForm } from "../lib/merge";
import { CheckMergeResponseSchema } from "~/generated/muxmultitool/merge/v1/merge_pb";

type Client = Parameters<typeof useMergeCheck>[1];
const formOf = (inputs: string[]): MergeForm => ({ inputs, dropStreams: [], chapters: false, output: "" });

describe("useMergeCheck", () => {
	beforeEach(() => vi.useFakeTimers());
	afterEach(() => vi.useRealTimers());

	it("checks two or more inputs after a pause and skips fewer", async () => {
		const checkMerge = vi.fn().mockResolvedValue(create(CheckMergeResponseSchema, { canMerge: true, command: "ffmpeg" }));
		const form = ref(formOf(["a"]));
		const merge = useMergeCheck(form, { checkMerge } as unknown as Client, 10);
		await vi.advanceTimersByTimeAsync(50);
		expect(checkMerge).not.toHaveBeenCalled();

		form.value = formOf(["a", "b"]);
		await nextTick();
		expect(merge.checking.value).toBe(true);
		await vi.advanceTimersByTimeAsync(50);
		expect(checkMerge).toHaveBeenCalledTimes(1);
		expect(checkMerge.mock.calls[0]?.[0].operation.inputs).toEqual(["a", "b"]);
		expect(merge.check.value?.command).toBe("ffmpeg");

		form.value = formOf(["a"]);
		await nextTick();
		expect(merge.check.value).toBeUndefined();
	});

	it("reports a refused check", async () => {
		const checkMerge = vi.fn().mockRejectedValue(new ConnectError("not a media file", Code.InvalidArgument));
		const merge = useMergeCheck(ref(formOf(["a", "b"])), { checkMerge } as unknown as Client, 10);
		await vi.advanceTimersByTimeAsync(50);
		expect(merge.error.value?.message).toBe("not a media file");
		expect(merge.check.value).toBeUndefined();
	});
});
