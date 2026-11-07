<script setup lang="ts">
import { CONTAINERS, type NameFields, renderTemplate } from "../lib/streams";

/**
 * The output options of the sketch: the target container (its extension picks it), the output folder
 * (empty puts the file next to its input) and the file name template, with the name the first file
 * would get. When extracting there is no container and the backend names the files, so only the
 * folder applies.
 */
const { exampleName, fields, extracting = false } = defineProps<{ exampleName?: string; fields?: NameFields; extracting?: boolean }>();
const container = defineModel<string>("container", { required: true });
const folder = defineModel<string>("folder", { required: true });
const template = defineModel<string>("template", { required: true });
const { t } = useI18n();

const example = computed(() => (exampleName ? renderTemplate(template.value, exampleName, container.value, fields) : undefined));
const field = "rounded-md border border-border bg-surface px-2 py-1 text-sm";
</script>

<template>
	<section class="flex flex-col gap-3" :aria-label="t('streams.output.title')">
		<h3 class="font-bold">{{ t("streams.output.title") }}</h3>
		<div class="flex flex-wrap gap-4">
			<label v-if="!extracting" class="flex flex-col gap-1 text-sm">
				{{ t("streams.output.container") }}
				<select v-model="container" :class="field">
					<option v-for="option in CONTAINERS" :key="option.extension" :value="option.extension">{{ option.name }}</option>
				</select>
			</label>
			<label class="flex min-w-64 flex-col gap-1 text-sm">
				{{ t("streams.output.folder") }}
				<input v-model="folder" type="text" :class="field" :placeholder="t('streams.output.sameAsSource')" />
				<span class="text-xs text-muted">{{ t("streams.output.folderHint") }}</span>
			</label>
			<label v-if="!extracting" class="flex min-w-64 flex-col gap-1 text-sm">
				{{ t("streams.output.template") }}
				<input v-model="template" type="text" :class="field" class="font-mono" />
				<span class="text-xs text-muted">
					{{ t("streams.output.templateHint") }}
					<template v-if="example">→ <span class="font-mono">{{ example }}</span></template>
				</span>
			</label>
			<p v-else class="max-w-prose self-end text-xs text-muted">{{ t("streams.output.extractNames") }}</p>
		</div>
	</section>
</template>
