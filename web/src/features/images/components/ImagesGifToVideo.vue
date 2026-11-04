<script setup lang="ts">
import { useImageJob } from "../composables/useImageJob";
import { gifVideoName, toGifToVideoOperation, videoEncoderFor } from "../lib/gif";
import type { ImageSupport } from "../lib/support";
import { GifVideoFormat } from "~/generated/muxmultitool/gif/v1/gif_pb";

/**
 * GIF → video panel (sketch 08): when the source is a GIF, MP4 (H.264) or WebM (VP9, else VP8)
 * instead of a GIF. A format whose encoder the build lacks is disabled with the reason.
 */
const { fileId, fileName, support } = defineProps<{ fileId: string; fileName: string; support: ImageSupport }>();
const { t } = useI18n();
const jobs = useJobsStore();
const { job, submitting, error, submit } = useImageJob((id) => jobs.byId(id));

const formats = computed(() =>
	(
		[
			{ value: GifVideoFormat.MP4, key: "mp4" },
			{ value: GifVideoFormat.WEBM, key: "webm" }
		] as const
	).map((entry) => ({ ...entry, encoder: videoEncoderFor(support, entry.value) }))
);
const format = ref<GifVideoFormat>(GifVideoFormat.MP4);
const chosen = computed(() => formats.value.find((entry) => entry.value === format.value));
const output = ref("");
const defaultOutput = computed(() => gifVideoName(fileName, format.value));
const outputName = computed(() => output.value.trim() || defaultOutput.value);

async function convert() {
	await submit({ case: "gifToVideo", value: toGifToVideoOperation(fileId, outputName.value, format.value) });
}
</script>

<template>
	<section class="flex flex-col gap-3" :aria-label="t('images.tabs.gif')">
		<p class="text-sm text-muted">{{ t("images.gif.toVideo") }}</p>

		<RekaRadioGroupRoot :model-value="format" class="flex flex-col gap-2 text-sm" @update:model-value="format = $event as GifVideoFormat">
			<span class="font-bold">{{ t("images.gif.format") }}</span>
			<RekaRadioGroupItem v-for="entry in formats" :key="entry.key" :value="entry.value" :disabled="entry.encoder === undefined" as-child>
				<button
					type="button"
					class="flex w-fit items-center gap-2 rounded-md border border-border px-3 py-1.5 hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent disabled:cursor-not-allowed disabled:opacity-50 data-[state=checked]:border-accent data-[state=checked]:bg-accent-surface"
				>
					{{ t(`images.gif.${entry.key}`) }}
					<span v-if="entry.encoder" class="text-xs text-muted">{{ t("images.gif.encoder", { encoder: entry.encoder }) }}</span>
					<span v-else class="text-xs text-danger">{{ t("images.gif.noEncoder", { format: t(`images.gif.${entry.key}`) }) }}</span>
				</button>
			</RekaRadioGroupItem>
		</RekaRadioGroupRoot>

		<label class="flex max-w-xl flex-col gap-1 text-sm">
			{{ t("images.output.label") }}
			<input v-model="output" type="text" class="rounded-md border border-border bg-surface px-2 py-1.5" :placeholder="defaultOutput" />
			<span class="text-xs text-muted">{{ t("images.output.hint") }}</span>
		</label>

		<div>
			<button
				type="button"
				class="rounded-md bg-accent px-4 py-1.5 text-sm text-on-accent focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-50"
				:disabled="submitting || chosen?.encoder === undefined"
				@click="convert"
			>
				{{ t("images.gif.convert") }}
			</button>
		</div>

		<ErrorPanel v-if="error" :error="error" />
		<ImagesOutput v-if="job" :job="job" :input-id="fileId" :output="outputName" />
	</section>
</template>
