<script setup lang="ts">
import { useImageJob } from "../composables/useImageJob";
import { coverName, toMs } from "../lib/outputs";
import { extensionOf } from "~/core/libs/fileNames";
import { formatTimecode } from "~/core/libs/format";
import { formatBytes } from "~/core/libs/jobFormat";
import { type FileReport, StreamType } from "~/generated/muxmultitool/files/v1/files_pb";

/**
 * Cover-art panel (sketch 08): what is embedded now, from the stream report (the video stream
 * flagged `attached_pic`), and a cover-art job that copies the file with the frame at the playhead
 * as its cover. Only MKV and MP4 can hold one; the output's extension decides.
 */
const { fileId, fileName, position, report } = defineProps<{
	fileId: string;
	fileName: string;
	position: number;
	report?: FileReport;
}>();
const { t } = useI18n();
const jobs = useJobsStore();
const { job, submitting, error, submit } = useImageJob((id) => jobs.byId(id));

const embedded = computed(() =>
	report?.streams.find((stream) => stream.type === StreamType.VIDEO && stream.flags.includes("attached_pic"))
);
const embeddedLabel = computed(() => {
	const stream = embedded.value;
	if (!stream) {
		return undefined;
	}
	const size = [
		stream.width && stream.height ? `${stream.width}×${stream.height}` : undefined,
		stream.sizeBytes > 0n ? formatBytes(stream.sizeBytes) : undefined
	]
		.filter(Boolean)
		.join(" · ");
	return t("images.cover.embedded", { codec: stream.codec, size });
});

const defaultOutput = computed(() => coverName(fileName));
const output = ref("");
const outputName = computed(() => output.value.trim() || defaultOutput.value);
const containerOk = computed(() => ["mkv", "mp4"].includes(extensionOf(outputName.value).toLowerCase()));

async function setCover() {
	await submit({ case: "coverArt", value: { input: fileId, output: outputName.value, positionMs: toMs(position) } });
}
</script>

<template>
	<section class="flex flex-col gap-3" :aria-label="t('images.tabs.cover')">
		<p class="text-sm text-muted">{{ t("images.cover.description") }}</p>
		<p class="text-sm">
			<span v-if="embeddedLabel">{{ embeddedLabel }}</span>
			<span v-else class="text-muted">{{ t("images.cover.none") }}</span>
		</p>
		<p class="text-sm">{{ t("images.position", { time: formatTimecode(position) }) }}</p>

		<label class="flex max-w-xl flex-col gap-1 text-sm">
			{{ t("images.output.label") }}
			<input v-model="output" type="text" class="rounded-md border border-border bg-surface px-2 py-1.5" :placeholder="defaultOutput" />
			<span class="text-xs text-muted">{{ t("images.output.hint") }}</span>
		</label>
		<p class="text-xs" :class="containerOk ? 'text-muted' : 'text-warning'">{{ t("images.cover.containers") }}</p>

		<div>
			<button
				type="button"
				class="rounded-md bg-accent px-4 py-1.5 text-sm text-on-accent focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-50"
				:disabled="submitting"
				@click="setCover"
			>
				{{ embedded ? t("images.cover.replace") : t("images.cover.set") }}
			</button>
		</div>

		<ErrorPanel v-if="error" :error="error" />
		<ImagesOutput v-if="job" :job="job" :input-id="fileId" :output="outputName" />
	</section>
</template>
