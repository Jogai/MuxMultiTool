<script setup lang="ts">
import { useImageJob } from "../composables/useImageJob";
import { frameUrl, snapshotName, toMs } from "../lib/outputs";
import type { ImageSupport } from "../lib/support";
import { formatTimecode } from "~/core/libs/format";

/**
 * Snapshot panel (sketch 08): the full-resolution frame at the playhead from the frame route, saved
 * as PNG or JPEG by a snapshot job. The buttons the build cannot serve (no png or mjpeg encoder) are
 * disabled with the reason.
 */
const { fileId, fileName, position, support } = defineProps<{
	fileId: string;
	fileName: string;
	/** The playhead in seconds. */
	position: number;
	support: ImageSupport;
}>();
const emit = defineEmits<{ cover: [] }>();
const { t } = useI18n();
const jobs = useJobsStore();
const { job, submitting, error, submit } = useImageJob((id) => jobs.byId(id));

// The preview follows the playhead with a pause, so scrubbing does not start a decoder per frame
const previewPosition = refDebounced(
	computed(() => position),
	300
);
const previewUrl = computed(() => frameUrl(fileId, previewPosition.value));
const previewState = ref<"loading" | "ok" | "error">("loading");
watch(previewUrl, () => {
	previewState.value = "loading";
});

const formats = computed(
	() =>
		[
			{ format: "png", key: "savePng", enabled: support.png, label: "PNG" },
			{ format: "jpg", key: "saveJpeg", enabled: support.jpeg, label: "JPEG" }
		] as const
);
const names = computed(() => ({ png: snapshotName(fileName, position, "png"), jpg: snapshotName(fileName, position, "jpg") }));
const lastOutput = ref("");

async function save(format: "png" | "jpg") {
	const output = names.value[format];
	lastOutput.value = output;
	await submit({ case: "snapshot", value: { input: fileId, output, positionMs: toMs(position) } });
}
</script>

<template>
	<section class="flex flex-col gap-3" :aria-label="t('images.tabs.snapshot')">
		<p class="text-sm text-muted">{{ t("images.snapshot.description") }}</p>
		<figure class="flex flex-col gap-1">
			<div class="preview relative flex aspect-video max-w-md items-center justify-center overflow-hidden rounded-md border border-border">
				<img
					:key="previewUrl"
					:src="previewUrl"
					:alt="t('images.snapshot.preview', { time: formatTimecode(previewPosition) })"
					class="max-h-full max-w-full"
					:class="{ invisible: previewState !== 'ok' }"
					@load="previewState = 'ok'"
					@error="previewState = 'error'"
				/>
				<p v-if="previewState !== 'ok'" class="absolute inset-0 flex items-center justify-center p-4 text-center text-sm text-muted">
					{{ previewState === "loading" ? t("images.snapshot.loading") : t("images.snapshot.previewFailed") }}
				</p>
			</div>
			<figcaption class="text-xs text-muted">{{ t("images.position", { time: formatTimecode(position) }) }}</figcaption>
		</figure>

		<div class="flex flex-wrap items-center gap-2">
			<button
				v-for="entry in formats"
				:key="entry.format"
				type="button"
				class="rounded-md bg-accent px-4 py-1.5 text-sm text-on-accent focus-visible:outline-2 focus-visible:outline-accent disabled:cursor-not-allowed disabled:opacity-50"
				:disabled="!entry.enabled || submitting"
				:title="entry.enabled ? names[entry.format] : t('images.snapshot.noEncoder', { format: entry.label })"
				@click="save(entry.format)"
			>
				{{ t(`images.snapshot.${entry.key}`) }}
			</button>
			<button
				type="button"
				class="rounded-md border border-border px-3 py-1.5 text-sm hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent"
				@click="emit('cover')"
			>
				{{ t("images.snapshot.setCover") }}
			</button>
		</div>
		<p class="text-xs text-muted">{{ t("images.snapshot.writes", { png: names.png, jpg: names.jpg }) }}</p>

		<ErrorPanel v-if="error" :error="error" />
		<ImagesOutput v-if="job" :job="job" :input-id="fileId" :output="lastOutput" />
	</section>
</template>

<style scoped>
/* The preview is picture, like the player's stage: black letterboxing in every theme */
.preview {
	background: #000;
}
</style>
