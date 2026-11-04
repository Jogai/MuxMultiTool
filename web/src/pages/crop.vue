<script setup lang="ts">
import { useCropPlan } from "~/features/images/composables/useCropPlan";
import { useOpenedFile } from "~/features/images/composables/useOpenedFile";
import { type CropValues, EMPTY_CROP } from "~/features/images/lib/crop";

/**
 * Crop screen (sketch 07): lossless crop of H.264 and H.265 video. The player over the file named by
 * the `file` query (`/crop?file=<id>`) or, without one, the first file of the selection, with the
 * draggable crop edges on top, and beside it the Crop panel. The crop values live here because the
 * overlay and the panel share them; the plan says whether the file can be cropped at all.
 */
const { t } = useI18n();
const { fileId, fileName, error, openFile } = useOpenedFile(["mkv", "mp4", "webm", "m2ts", "ts", "mov", "avi"]);

const position = ref(0);
const crop = ref<CropValues>(EMPTY_CROP);
const output = ref("");
const cropPlan = useCropPlan(() => (fileId.value ? { input: fileId.value, output: output.value.trim(), crop: crop.value } : undefined));
watch(fileId, () => {
	crop.value = EMPTY_CROP;
	output.value = "";
	position.value = 0;
});
</script>

<template>
	<section class="flex flex-col gap-4 p-6">
		<header class="flex flex-wrap items-center gap-3">
			<h2 class="text-xl font-bold">{{ t("nav.crop") }}</h2>
			<span v-if="fileId" class="truncate text-sm text-muted">{{ fileName }}</span>
			<button
				type="button"
				class="rounded-md bg-accent px-3 py-1.5 text-sm text-on-accent focus-visible:outline-2 focus-visible:outline-accent"
				@click="openFile"
			>
				{{ t("images.open") }}
			</button>
		</header>
		<ErrorPanel v-if="error" :error="error" class="max-w-prose" />

		<div v-if="fileId" class="grid gap-6 xl:grid-cols-[minmax(0,3fr)_minmax(20rem,2fr)]">
			<PlayerView :key="fileId" :file-id="fileId" class="min-w-0" @timeupdate="position = $event">
				<template #overlay>
					<ImagesCropOverlay v-if="cropPlan.source.value" v-model:crop="crop" :source="cropPlan.source.value" />
				</template>
			</PlayerView>

			<ImagesCrop
				v-model:crop="crop"
				v-model:output="output"
				class="min-w-0"
				:file-id="fileId"
				:file-name="fileName"
				:position="position"
				:plan="cropPlan.plan.value"
				:planning="cropPlan.planning.value"
				:plan-error="cropPlan.error.value"
			/>
		</div>
		<template v-else>
			<p class="max-w-prose text-muted">{{ t("pages.crop.description") }}</p>
			<p class="max-w-prose text-sm text-muted">{{ t("pages.crop.empty") }}</p>
		</template>
	</section>
</template>
