import { create } from "@bufbuild/protobuf";
import { Code, ConnectError } from "@connectrpc/connect";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { nextTick, ref } from "vue";

import { useCombinePreview } from "./useCombinePreview";
import { type CombineForm, emptyCombineForm } from "../lib/combine";
import { PreviewCombineResponseSchema } from "~/generated/muxmultitool/combine/v1/combine_pb";

type Client = Parameters<typeof useCombinePreview>[1];
const formWith = (tracks: CombineForm["tracks"], offset = ""): CombineForm => ({
	...emptyCombineForm(),
	sources: [
		{ id: "a", name: "a.mkv", offset },
		{ id: "b", name: "b.mkv", offset: "" }
	],
	tracks
});

describe("useCombinePreview", () => {
	beforeEach(() => vi.useFakeTimers());
	afterEach(() => vi.useRealTimers());

	it("previews once there is a track, and clears without one", async () => {
		const previewCombine = vi.fn().mockResolvedValue(create(PreviewCombineResponseSchema, { canRun: true, command: "ffmpeg" }));
		const form = ref(formWith([]));
		const combine = useCombinePreview(form, { previewCombine } as unknown as Client, 10);
		await vi.advanceTimersByTimeAsync(50);
		expect(previewCombine).not.toHaveBeenCalled();

		form.value = formWith([{ source: 1, streamIndex: 2 }]);
		await nextTick();
		expect(combine.checking.value).toBe(true);
		await vi.advanceTimersByTimeAsync(50);
		expect(previewCombine).toHaveBeenCalledTimes(1);
		expect(previewCombine.mock.calls[0]?.[0].operation.tracks[0]).toMatchObject({ source: 1, streamIndex: 2 });
		expect(combine.preview.value?.command).toBe("ffmpeg");

		// An offset that is not a number holds the preview back instead of sending 0
		form.value = formWith([{ source: 1, streamIndex: 2 }], "abc");
		await vi.advanceTimersByTimeAsync(50);
		expect(previewCombine).toHaveBeenCalledTimes(1);
		expect(combine.preview.value).toBeUndefined();
	});

	it("reports a refused preview", async () => {
		const previewCombine = vi.fn().mockRejectedValue(new ConnectError("source B is not a media file", Code.InvalidArgument));
		const combine = useCombinePreview(ref(formWith([{ source: 0, streamIndex: 0 }])), { previewCombine } as unknown as Client, 10);
		await vi.advanceTimersByTimeAsync(50);
		expect(combine.error.value?.message).toBe("source B is not a media file");
		expect(combine.preview.value).toBeUndefined();
	});
});
