<script setup lang="ts">
import type { AppError } from "~/core/libs/errors";
import type { Entry } from "~/generated/muxmultitool/files/v1/files_pb";

/**
 * Step 1 of the sketch: the folder to search, the wildcard patterns and whether subfolders count,
 * with how many files match right now and, on request, their list.
 */
const {
	matches,
	checking = false,
	error
} = defineProps<{
	/** The files the patterns match, once looked up. */
	matches?: readonly Entry[];
	checking?: boolean;
	error?: AppError;
}>();
const folder = defineModel<string>("folder", { required: true });
const folderLabel = defineModel<string>("folderLabel", { required: true });
const patterns = defineModel<string>("patterns", { required: true });
const recursive = defineModel<boolean>("recursive", { required: true });
const { t } = useI18n();
const listOpen = ref(false);

const field = "rounded-md border border-border bg-surface px-2 py-1 text-sm";
</script>

<template>
	<section class="flex flex-col gap-3" :aria-label="t('batch.steps.input')">
		<h3 class="font-bold"><span class="mr-2 rounded-full bg-accent px-2 text-sm text-on-accent">1</span>{{ t("batch.steps.input") }}</h3>
		<div class="flex flex-wrap items-start gap-4">
			<BatchFolderField
				v-model:id="folder"
				v-model:name="folderLabel"
				:label="t('batch.input.folder')"
				:placeholder="t('batch.folder.none')"
			/>
			<label class="flex min-w-48 flex-col gap-1 text-sm">
				{{ t("batch.input.pattern") }}
				<input v-model="patterns" type="text" :class="field" class="font-mono" :placeholder="'*.mp4'" />
				<span class="text-xs text-muted">{{ t("batch.input.patternHint") }}</span>
			</label>
			<label class="flex items-center gap-2 self-end pb-1 text-sm">
				<input v-model="recursive" type="checkbox" class="accent-accent" />
				{{ t("batch.input.recursive") }}
			</label>
		</div>
		<ErrorPanel v-if="error" :error="error" />
		<p v-else-if="folder" class="flex flex-wrap items-center gap-3 text-sm" role="status">
			<span v-if="checking" class="text-muted">{{ t("batch.input.checking") }}</span>
			<template v-else-if="matches">
				<span v-if="matches.length === 0" class="text-muted">{{ t("batch.input.noMatches") }}</span>
				<span v-else>{{ t("batch.input.matches", { n: matches.length }, matches.length) }}</span>
				<button v-if="matches.length > 0" type="button" class="text-accent underline" @click="listOpen = !listOpen">
					{{ listOpen ? t("batch.input.hideList") : t("batch.input.showList") }}
				</button>
			</template>
		</p>
		<ul
			v-if="listOpen && matches && matches.length > 0"
			class="max-h-48 overflow-y-auto rounded-md border border-border p-2 font-mono text-xs"
		>
			<li v-for="file in matches" :key="file.id" class="truncate" :title="file.path">{{ file.path || file.name }}</li>
		</ul>
	</section>
</template>
