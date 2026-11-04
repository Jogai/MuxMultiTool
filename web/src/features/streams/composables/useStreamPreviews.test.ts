import { create } from "@bufbuild/protobuf";
import { Code, ConnectError } from "@connectrpc/connect";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { nextTick, ref } from "vue";

import { type PreviewClients, useStreamPreviews } from "./useStreamPreviews";
import { emptyForm, type StreamsForm } from "../lib/streams";
import { PreviewExtractResponseSchema } from "~/generated/muxmultitool/extract/v1/extract_pb";
import { type Entry, EntryKind, EntrySchema, StreamType } from "~/generated/muxmultitool/files/v1/files_pb";
import { PreviewStreamsResponseSchema, SelectionMode } from "~/generated/muxmultitool/streams/v1/streams_pb";

const entry = (id: string): Entry => create(EntrySchema, { id, name: `${id}.mkv`, kind: EntryKind.FILE });

const basePreview = create(PreviewStreamsResponseSchema, {
	command: "ffmpeg base",
	canRun: true,
	streams: [
		{ stream: { index: 0, type: StreamType.VIDEO }, kept: true, why: "no rule applies" },
		{ stream: { index: 1, type: StreamType.AUDIO }, kept: true, why: "no rule applies" }
	]
});

function clients(
	previewStreams = vi.fn(),
	previewExtract = vi.fn()
): PreviewClients & { previewStreams: typeof previewStreams; previewExtract: typeof previewExtract } {
	return {
		streams: { previewStreams } as unknown as PreviewClients["streams"],
		extract: { previewExtract } as unknown as PreviewClients["extract"],
		previewStreams,
		previewExtract
	};
}

describe("useStreamPreviews", () => {
	beforeEach(() => vi.useFakeTimers());
	afterEach(() => vi.useRealTimers());

	it("previews every file after a pause and only the changed ones again", async () => {
		const fake = clients(vi.fn().mockResolvedValue(basePreview));
		const form = ref<StreamsForm>(emptyForm());
		const files = ref<Entry[]>([entry("a"), entry("b")]);
		const previews = useStreamPreviews(form, files, fake, 10);
		expect(previews.loading.value).toBe(true);
		await vi.advanceTimersByTimeAsync(50);
		expect(fake.previewStreams).toHaveBeenCalledTimes(2);
		expect(previews.previewOf("a")?.effective?.command).toBe("ffmpeg base");
		expect(previews.loading.value).toBe(false);

		// A tick on "a" reruns "a" alone: the base preview, then the KEEP of what is left
		form.value = { ...form.value, overrides: new Map([["a", new Set([1])]]) };
		await nextTick();
		expect(previews.previewOf("a")?.loading).toBe(true);
		expect(previews.previewOf("b")?.loading).toBe(false);
		await vi.advanceTimersByTimeAsync(50);
		expect(fake.previewStreams).toHaveBeenCalledTimes(4);
		const keep = fake.previewStreams.mock.calls[3]?.[0].operation;
		expect(keep.mode).toBe(SelectionMode.KEEP);
		expect(keep.streamIndexes).toEqual([0]);

		// A file that leaves the selection is forgotten
		files.value = [entry("a")];
		await vi.advanceTimersByTimeAsync(50);
		expect(previews.previewOf("b")).toBeUndefined();
		expect(fake.previewStreams).toHaveBeenCalledTimes(4);
	});

	it("asks the extract service when extracting", async () => {
		const fake = clients(
			vi.fn(),
			vi.fn().mockResolvedValue(create(PreviewExtractResponseSchema, { command: "ffmpeg extract", canRun: true }))
		);
		const form = ref<StreamsForm>({ ...emptyForm(), action: "extract" });
		const previews = useStreamPreviews(form, ref([entry("a")]), fake, 10);
		await vi.advanceTimersByTimeAsync(50);
		expect(fake.previewStreams).not.toHaveBeenCalled();
		expect(previews.previewOf("a")?.extract?.command).toBe("ffmpeg extract");
	});

	it("keeps a refused preview as the file's error", async () => {
		const fake = clients(vi.fn().mockRejectedValue(new ConnectError("no such file", Code.NotFound)));
		const previews = useStreamPreviews(ref(emptyForm()), ref([entry("a")]), fake, 10);
		await vi.advanceTimersByTimeAsync(50);
		expect(previews.previewOf("a")?.error?.message).toBe("no such file");
		expect(previews.previewOf("a")?.effective).toBeUndefined();
	});
});
