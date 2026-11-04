<script setup lang="ts">
import { batchTemplateOf, containerName, rulesSummary } from "../lib/presets";
import { streamTypeName } from "~/core/libs/reportFormat";
import { StreamType } from "~/generated/muxmultitool/files/v1/files_pb";
import type { Preset } from "~/generated/muxmultitool/presets/v1/presets_pb";
import { UntaggedLanguage } from "~/generated/muxmultitool/streams/v1/streams_pb";

/**
 * One preset of the list (sketch 09): its name, what it does in a line, and the actions: apply it to
 * the selected files (into the Streams screen) or to a batch, edit it, delete it.
 */
const { preset, fileCount } = defineProps<{ preset: Preset; fileCount: number }>();
const emit = defineEmits<{ applyFiles: []; applyBatch: []; edit: []; delete: [] }>();
const { t } = useI18n();

const typeName = (type: StreamType) => (type === StreamType.UNSPECIFIED ? t("streams.rules.anyType") : streamTypeName(type));
const rules = computed(() => rulesSummary(preset.settings, typeName));
const ruleCount = computed(() => preset.settings?.rules?.rules.length ?? 0);
const untaggedDropped = computed(() => preset.settings?.rules?.untagged === UntaggedLanguage.DROP);
const template = computed(() => (preset.settings ? batchTemplateOf(preset.settings) : ""));
const container = computed(() => containerName(preset.settings?.container ?? 0));

const button =
	"rounded-md border border-border px-3 py-1 text-sm hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-50";
</script>

<template>
	<li class="flex flex-col gap-2 rounded-md border border-border p-3">
		<div class="flex flex-wrap items-baseline gap-x-3 gap-y-1">
			<h3 class="font-bold">{{ preset.name }}</h3>
			<span class="text-sm text-muted">{{ container }} · {{ t("presets.card.rules", { n: ruleCount }, ruleCount) }}</span>
		</div>
		<p class="text-sm text-muted">
			<span v-if="rules" class="font-mono">{{ rules }}</span>
			<span v-else>{{ t("presets.card.noRules") }}</span>
			<span v-if="untaggedDropped"> · {{ t("presets.card.untaggedDrop") }}</span>
			<span> · {{ t("presets.card.template", { template }) }}</span>
		</p>
		<div class="flex flex-wrap items-center gap-2">
			<ShellTooltip :text="fileCount === 0 ? t('presets.actions.applyFilesHint') : undefined">
				<button
					type="button"
					class="rounded-md bg-accent px-3 py-1 text-sm text-on-accent focus-visible:outline-2 focus-visible:outline-accent aria-disabled:cursor-not-allowed aria-disabled:opacity-50"
					:aria-disabled="fileCount === 0 || undefined"
					@click="fileCount > 0 && emit('applyFiles')"
				>
					{{ t("presets.actions.applyFiles", { n: fileCount }, fileCount) }}
				</button>
			</ShellTooltip>
			<button type="button" :class="button" @click="emit('applyBatch')">{{ t("presets.actions.applyBatch") }}</button>
			<span class="ml-auto flex gap-2">
				<button type="button" :class="button" @click="emit('edit')">{{ t("presets.actions.edit") }}</button>
				<button type="button" :class="button" class="text-danger" @click="emit('delete')">{{ t("presets.actions.delete") }}</button>
			</span>
		</div>
	</li>
</template>
