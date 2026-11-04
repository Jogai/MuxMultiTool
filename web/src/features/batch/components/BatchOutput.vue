<script setup lang="ts">
import { placeholdersOf, type PreviewFile, previewNames, type TemplateProblem, templateTarget } from "../lib/template";

/**
 * Step 3 of the sketch: the output folder (empty puts every output next to its input), the filename
 * template with the names the first matching files would get, and what to do with an output that
 * exists. With a preset as the operation the preset names the outputs, so the template is shown,
 * not edited.
 */
const {
	presetTemplate,
	files = [],
	folderName = "",
	problem,
	date
} = defineProps<{
	/** The preset's template, when the operation is a preset. */
	presetTemplate?: string;
	/** The files the patterns match, for the preview. */
	files?: readonly PreviewFile[];
	/** The searched folder's name, the `{parent}` of a file at its top. */
	folderName?: string;
	problem?: TemplateProblem;
	/** The `{date}` of the preview; today when absent. */
	date?: string;
}>();
const outputFolder = defineModel<string>("outputFolder", { required: true });
const outputFolderLabel = defineModel<string>("outputFolderLabel", { required: true });
const template = defineModel<string>("template", { required: true });
const overwrite = defineModel<boolean>("overwrite", { required: true });
const { t } = useI18n();

const PREVIEW_LINES = 5;
const effectiveTemplate = computed(() => presetTemplate ?? template.value.trim());
const names = computed(() => (problem ? [] : previewNames(effectiveTemplate.value, files, { folderName, date }, PREVIEW_LINES)));
const more = computed(() => Math.max(0, files.length - PREVIEW_LINES));
const usesLang = computed(() => !problem && placeholdersOf(effectiveTemplate.value).includes("lang"));
const target = computed(() => (problem ? undefined : templateTarget(effectiveTemplate.value)));
const targetNote = computed(() => {
	if (problem) {
		return undefined;
	}
	if (target.value === undefined) {
		return t("batch.output.target.unknown");
	}
	return target.value === "input" ? t("batch.output.target.input") : t("batch.output.target.container", { container: target.value });
});
const problemText = computed(() => {
	if (!problem) {
		return undefined;
	}
	return problem.kind === "unknown"
		? t("batch.output.problems.template.unknown", { placeholder: problem.placeholder })
		: t(`batch.output.problems.template.${problem.kind}`);
});

const field = "rounded-md border border-border bg-surface px-2 py-1 text-sm";
const cell = "py-1 pr-3 align-top";
</script>

<template>
	<section class="flex flex-col gap-3" :aria-label="t('batch.steps.output')">
		<h3 class="font-bold"><span class="mr-2 rounded-full bg-accent px-2 text-sm text-on-accent">3</span>{{ t("batch.steps.output") }}</h3>
		<div class="flex flex-wrap items-start gap-4">
			<BatchFolderField
				v-model:id="outputFolder"
				v-model:name="outputFolderLabel"
				:label="t('batch.output.folder')"
				:placeholder="t('batch.output.sameAsSource')"
				clearable
			/>
			<label class="flex min-w-64 flex-col gap-1 text-sm">
				{{ t("batch.output.template") }}
				<template v-if="presetTemplate === undefined">
					<input v-model="template" type="text" :class="field" class="font-mono" :aria-invalid="problem !== undefined" />
					<span class="text-xs text-muted">{{ t("batch.output.templateHint") }}</span>
				</template>
				<template v-else>
					<span :class="field" class="font-mono text-muted">{{ presetTemplate }}</span>
					<span class="text-xs text-muted">{{ t("batch.output.presetTemplate", { template: presetTemplate }) }}</span>
				</template>
				<span v-if="problemText" class="text-xs text-danger" role="alert">{{ problemText }}</span>
				<span v-else-if="targetNote" class="text-xs" :class="target === undefined ? 'text-warning' : 'text-muted'">{{ targetNote }}</span>
			</label>
			<fieldset class="flex flex-col gap-1 text-sm">
				<legend class="mb-1">{{ t("batch.output.existing") }}</legend>
				<label class="flex items-center gap-2">
					<input v-model="overwrite" type="radio" :value="false" class="accent-accent" />
					{{ t("batch.output.skip") }}
				</label>
				<label class="flex items-center gap-2">
					<input v-model="overwrite" type="radio" :value="true" class="accent-accent" />
					{{ t("batch.output.overwrite") }}
				</label>
			</fieldset>
		</div>

		<div class="flex flex-col gap-1" :aria-label="t('batch.output.preview')">
			<h4 class="text-sm font-bold">{{ t("batch.output.preview") }}</h4>
			<p v-if="names.length === 0" class="text-sm text-muted">{{ t("batch.output.noFiles") }}</p>
			<table v-else class="w-full max-w-3xl border-collapse text-sm">
				<thead class="text-left text-xs text-muted uppercase">
					<tr class="border-b border-border">
						<th scope="col" class="py-1 pr-3 font-normal">{{ t("batch.output.previewInput") }}</th>
						<th scope="col" class="py-1 font-normal">{{ t("batch.output.previewOutput") }}</th>
					</tr>
				</thead>
				<tbody>
					<tr v-for="line in names" :key="line.input" class="border-b border-border font-mono">
						<td :class="cell">{{ line.input }}</td>
						<td class="py-1 align-top">{{ line.output }}</td>
					</tr>
					<tr v-if="more > 0">
						<td colspan="2" class="py-1 text-muted">{{ t("batch.output.more", { n: more }) }}</td>
					</tr>
				</tbody>
			</table>
			<p v-if="names.length > 0 && usesLang" class="text-xs text-muted">{{ t("batch.output.langNote") }}</p>
		</div>
	</section>
</template>
