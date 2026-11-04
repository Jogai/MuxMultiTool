<script setup lang="ts">
import { useImageSupport } from "~/features/images/composables/useImageSupport";
import { useOpenedFile } from "~/features/images/composables/useOpenedFile";
import { isGifSource } from "~/features/images/lib/gif";
import { useFileStreams } from "~/features/player/composables/useFileStreams";
import type { PlayerRange } from "~/features/player/lib/range";

/**
 * Images screen (sketch 08): the player over the file named by the `file` query
 * (`/images?file=<id>`) or, without one, the first file of the selection, and beside it the tabs that
 * share its playhead and range: Snapshot, Cover art and GIF (or GIF → video for a GIF source).
 * Lossless crop has its own screen.
 */
const { t } = useI18n();
const { fileId, fileName, error, openFile } = useOpenedFile(["mkv", "mp4", "webm", "m2ts", "ts", "mov", "avi", "gif"]);

const streams = useFileStreams(() => fileId.value);
const duration = computed(() => streams.duration.value ?? 0);
const gifSource = computed(() => isGifSource(streams.report.value));
const { support, unavailable, error: supportError } = useImageSupport();

// What the player hands to the tabs
const range = ref<PlayerRange>({});
const position = ref(0);

type Tab = "snapshot" | "cover" | "gif";
const tab = ref<Tab>("snapshot");
const tabs: readonly Tab[] = ["snapshot", "cover", "gif"];
watch(fileId, () => {
	range.value = {};
	position.value = 0;
});
</script>

<template>
	<section class="flex flex-col gap-4 p-6">
		<header class="flex flex-wrap items-center gap-3">
			<h2 class="text-xl font-bold">{{ t("nav.images") }}</h2>
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
		<ErrorPanel v-else-if="supportError" :error="supportError" class="max-w-prose" />
		<p v-if="unavailable" class="max-w-prose text-sm text-danger">{{ t("images.ffmpegUnavailable", { reason: unavailable }) }}</p>

		<div v-if="fileId" class="grid gap-6 xl:grid-cols-[minmax(0,3fr)_minmax(20rem,2fr)]">
			<PlayerView :key="fileId" v-model:range="range" :file-id="fileId" class="min-w-0" @timeupdate="position = $event" />

			<RekaTabsRoot v-model="tab" class="flex min-w-0 flex-col gap-4">
				<RekaTabsList class="flex gap-1 border-b border-border">
					<RekaTabsTrigger v-for="name in tabs" :key="name" :value="name" as-child>
						<button
							type="button"
							class="-mb-px border-b-2 border-transparent px-3 py-2 text-sm text-muted data-[state=active]:border-accent data-[state=active]:text-text focus-visible:outline-2 focus-visible:outline-accent"
						>
							{{ t(`images.tabs.${name}`) }}
						</button>
					</RekaTabsTrigger>
				</RekaTabsList>

				<RekaTabsContent value="snapshot">
					<ImagesSnapshot :file-id="fileId" :file-name="fileName" :position="position" :support="support" @cover="tab = 'cover'" />
				</RekaTabsContent>
				<RekaTabsContent value="cover">
					<ImagesCoverArt :file-id="fileId" :file-name="fileName" :position="position" :report="streams.report.value" />
				</RekaTabsContent>
				<RekaTabsContent value="gif">
					<ImagesGifToVideo v-if="gifSource" :file-id="fileId" :file-name="fileName" :support="support" />
					<ImagesGif v-else :file-id="fileId" :file-name="fileName" :range="range" :duration="duration" :support="support" />
				</RekaTabsContent>
			</RekaTabsRoot>
		</div>
		<template v-else>
			<p class="max-w-prose text-muted">{{ t("pages.images.description") }}</p>
			<p class="max-w-prose text-sm text-muted">{{ t("images.empty") }}</p>
		</template>
	</section>
</template>
