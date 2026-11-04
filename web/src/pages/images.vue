<script setup lang="ts">
import { useCropPlan } from "~/features/images/composables/useCropPlan";
import { useImageSupport } from "~/features/images/composables/useImageSupport";
import { type CropValues, EMPTY_CROP } from "~/features/images/lib/crop";
import { isGifSource } from "~/features/images/lib/gif";
import { useFileStreams } from "~/features/player/composables/useFileStreams";
import { entriesForPicked } from "~/core/libs/pickedFiles";
import { type AppError, toAppError } from "~/core/libs/errors";
import { useRpcClient } from "~/core/libs/rpc";
import type { PlayerRange } from "~/features/player/lib/range";
import { FilesService } from "~/generated/muxmultitool/files/v1/files_pb";

/**
 * Images screen (sketches 07 and 08): the player over the file named by the `file` query
 * (`/images?file=<id>`) or, without one, the first file of the selection, and beside it the tabs that
 * share its playhead and range: Snapshot, Cover art, GIF (or GIF → video for a GIF source) and Crop.
 * The crop values live here because the overlay on the player and the Crop panel share them, and
 * the crop plan's support answer is what disables the Crop tab for codecs other than H.264/H.265.
 */
const { t } = useI18n();
const route = useRoute();
const router = useRouter();
const picker = useFilePicker();
const selection = useFileSelectionStore();
const filesClient = useRpcClient(FilesService);

const fileId = computed(() => (typeof route.query.file === "string" && route.query.file !== "" ? route.query.file : selection.ids[0]));
const entryName = ref<string>();
watch(
	fileId,
	async (id) => {
		entryName.value = undefined;
		if (!id) {
			return;
		}
		const known = selection.files.find((file) => file.id === id);
		if (known) {
			entryName.value = known.name;
			return;
		}
		try {
			const response = await filesClient.getEntries({ ids: [id] });
			entryName.value = response.entries[0]?.name || undefined;
		} catch {
			// The player's report shows the error; the name falls back to the id's last segment
		}
	},
	{ immediate: true }
);
const fileName = computed(() => entryName.value ?? baseName(fileId.value ?? ""));

const streams = useFileStreams(() => fileId.value);
const duration = computed(() => streams.duration.value ?? 0);
const gifSource = computed(() => isGifSource(streams.report.value));
const { support, unavailable, error: supportError } = useImageSupport();

// What the player hands to the tabs
const range = ref<PlayerRange>({});
const position = ref(0);

type Tab = "snapshot" | "cover" | "gif" | "crop";
const tab = ref<Tab>("snapshot");
const tabs: readonly Tab[] = ["snapshot", "cover", "gif", "crop"];

// The crop: values and output shared by the overlay and the panel, planned on every change
const crop = ref<CropValues>(EMPTY_CROP);
const cropOutput = ref("");
const cropPlan = useCropPlan(() => (fileId.value ? { input: fileId.value, output: cropOutput.value.trim(), crop: crop.value } : undefined));
const cropDisabled = computed(() => cropPlan.supported.value === false);
watch(fileId, () => {
	crop.value = EMPTY_CROP;
	cropOutput.value = "";
	range.value = {};
	position.value = 0;
	if (tab.value === "crop") {
		tab.value = "snapshot";
	}
});
watch(cropDisabled, (disabled) => {
	if (disabled && tab.value === "crop") {
		tab.value = "snapshot";
	}
});

const error = ref<AppError>();
async function openFile() {
	error.value = undefined;
	try {
		const picked = await picker.pickFiles({ multiple: false, extensions: ["mkv", "mp4", "webm", "m2ts", "ts", "mov", "avi", "gif"] });
		const [entry] = await entriesForPicked(filesClient, picker.mode, picked);
		const id = entry?.id ?? picked[0]?.path;
		if (id) {
			await router.replace({ query: { ...route.query, file: id } });
		}
	} catch (cause) {
		error.value = toAppError(cause);
	}
}
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
			<PlayerView :key="fileId" v-model:range="range" :file-id="fileId" class="min-w-0" @timeupdate="position = $event">
				<template #overlay>
					<ImagesCropOverlay v-if="tab === 'crop' && cropPlan.source.value" v-model:crop="crop" :source="cropPlan.source.value" />
				</template>
			</PlayerView>

			<RekaTabsRoot v-model="tab" class="flex min-w-0 flex-col gap-4">
				<RekaTabsList class="flex gap-1 border-b border-border">
					<RekaTabsTrigger v-for="name in tabs" :key="name" :value="name" :disabled="name === 'crop' && cropDisabled" as-child>
						<button
							type="button"
							class="-mb-px border-b-2 border-transparent px-3 py-2 text-sm text-muted data-[state=active]:border-accent data-[state=active]:text-text focus-visible:outline-2 focus-visible:outline-accent disabled:cursor-not-allowed disabled:opacity-50"
							:title="name === 'crop' && cropDisabled ? t('images.cropDisabled', { reason: cropPlan.reason.value }) : undefined"
						>
							{{ t(`images.tabs.${name}`) }}
						</button>
					</RekaTabsTrigger>
				</RekaTabsList>
				<p v-if="cropDisabled" class="text-xs text-muted">{{ t("images.cropDisabled", { reason: cropPlan.reason.value }) }}</p>

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
				<RekaTabsContent value="crop">
					<ImagesCrop
						v-model:crop="crop"
						v-model:output="cropOutput"
						:file-id="fileId"
						:file-name="fileName"
						:position="position"
						:plan="cropPlan.plan.value"
						:planning="cropPlan.planning.value"
						:plan-error="cropPlan.error.value"
					/>
				</RekaTabsContent>
			</RekaTabsRoot>
		</div>
		<template v-else>
			<p class="max-w-prose text-muted">{{ t("pages.images.description") }}</p>
			<p class="max-w-prose text-sm text-muted">{{ t("images.empty") }}</p>
		</template>
	</section>
</template>
