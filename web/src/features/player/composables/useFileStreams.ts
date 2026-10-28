import { Code } from "@connectrpc/connect";
import { computed, type MaybeRefOrGetter, onScopeDispose, ref, shallowRef, toValue, watch } from "vue";

import { type AppError, codeName, toAppError } from "~/core/libs/errors";
import { useRpcClient } from "~/core/libs/rpc";
import { type FileReport, FilesService, StreamType } from "~/generated/muxmultitool/files/v1/files_pb";

import { subtitleStreams } from "../lib/cues";

/**
 * The stream report of one file, as the player needs it: the frame rate of the first video stream for
 * frame stepping, and the subtitle streams for the track selection. A quick report, so it takes only as
 * long as reading the file headers.
 */
export function useFileStreams(fileId: MaybeRefOrGetter<string | undefined>) {
	const client = useRpcClient(FilesService);
	const report = shallowRef<FileReport>();
	const error = ref<AppError>();
	const loading = ref(false);
	let controller: AbortController | undefined;

	async function load(): Promise<void> {
		controller?.abort();
		controller = undefined;
		report.value = undefined;
		error.value = undefined;
		const id = toValue(fileId);
		if (!id) {
			return;
		}
		const mine = new AbortController();
		controller = mine;
		loading.value = true;
		try {
			const response = await client.getStreamReport({ fileIds: [id], quick: true }, { signal: mine.signal });
			if (mine.signal.aborted) {
				return;
			}
			const file = response.files[0];
			if (file?.error) {
				// The report carries the probe failure per file instead of failing the call
				error.value = {
					code: Code.FailedPrecondition,
					codeName: codeName(Code.FailedPrecondition),
					message: file.error.message,
					raw: file.error.ffprobeOutput || undefined,
					cause: file.error
				};
			}
			report.value = file;
		} catch (cause) {
			if (!mine.signal.aborted) {
				error.value = toAppError(cause);
			}
		} finally {
			if (controller === mine) {
				loading.value = false;
			}
		}
	}

	const frameRate = computed(() => {
		const video = report.value?.streams.find((stream) => stream.type === StreamType.VIDEO && stream.frameRate > 0);
		return video?.frameRate;
	});
	const subtitles = computed(() => subtitleStreams(report.value?.streams ?? []));
	const duration = computed(() => report.value?.container?.durationSeconds);

	watch(
		() => toValue(fileId),
		() => void load(),
		{ immediate: true }
	);
	onScopeDispose(() => controller?.abort());

	return { report, frameRate, subtitles, duration, error, loading, reload: load };
}
