<script setup lang="ts">
import type { Preset } from "~/generated/muxmultitool/presets/v1/presets_pb";
import { usePresets } from "~/features/presets/composables/usePresets";
import { toStreamsForm } from "~/features/presets/lib/presets";

/**
 * The Presets screen (sketch 09): every saved preset with what it does, applied to the selected files
 * (its rules, container and output name go into the Streams screen) or to a batch (it becomes the
 * Batch screen's operation), edited or deleted. "Save as preset…" on the Streams screen adds to the
 * list; "New preset" here starts one from scratch.
 */
const { t } = useI18n();
const router = useRouter();
const selection = useFileSelectionStore();
const streamsForm = useStreamsFormStore();
const batch = useBatchStore();
const presets = usePresets();
onMounted(() => void presets.load());

const editOpen = ref(false);
const editing = ref<Preset>();
const deleting = ref<Preset>();

function create() {
	editing.value = undefined;
	editOpen.value = true;
}

function edit(preset: Preset) {
	editing.value = preset;
	editOpen.value = true;
}

async function applyToFiles(preset: Preset) {
	streamsForm.applyPreset(toStreamsForm(preset));
	await router.push("/streams");
}

async function applyToBatch(preset: Preset) {
	batch.applyPreset(preset.id);
	await router.push("/batch");
}
</script>

<template>
	<section class="flex flex-col gap-4 p-6">
		<header class="flex flex-wrap items-start gap-4">
			<div class="min-w-0 flex-1">
				<h2 class="text-xl font-bold">{{ t("nav.presets") }}</h2>
				<p class="mt-1 max-w-prose text-sm text-muted">{{ t("presets.description") }}</p>
			</div>
			<button
				type="button"
				class="rounded-md bg-accent px-3 py-1.5 text-sm text-on-accent focus-visible:outline-2 focus-visible:outline-accent"
				@click="create"
			>
				{{ t("presets.new") }}
			</button>
		</header>

		<ErrorPanel v-if="presets.error.value" :error="presets.error.value" />
		<p v-else-if="presets.loading.value && !presets.loaded.value" class="text-sm text-muted">{{ t("presets.loading") }}</p>
		<p v-else-if="presets.presets.value.length === 0" class="max-w-prose text-sm text-muted">{{ t("presets.empty") }}</p>
		<ul v-else class="flex max-w-4xl flex-col gap-3">
			<PresetsCard
				v-for="preset in presets.presets.value"
				:key="preset.id"
				:preset="preset"
				:file-count="selection.count"
				@apply-files="applyToFiles(preset)"
				@apply-batch="applyToBatch(preset)"
				@edit="edit(preset)"
				@delete="deleting = preset"
			/>
		</ul>

		<PresetsEditDialog v-model:open="editOpen" :preset="editing" @saved="presets.replace" />
		<PresetsDeleteDialog v-model="deleting" @deleted="presets.remove" />
	</section>
</template>
