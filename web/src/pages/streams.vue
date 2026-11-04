<script setup lang="ts">
import IconKeyboard from "~icons/lucide/keyboard";
import IconStar from "~icons/lucide/star";

import { useStreamReport } from "~/core/composables/useStreamReport";
import { type AppError, toAppError } from "~/core/libs/errors";
import { entriesForPicked } from "~/core/libs/pickedFiles";
import { useRpcClient } from "~/core/libs/rpc";
import type { FileSummary } from "~/features/streams/components/StreamsFileList.vue";
import type { TableRow } from "~/features/streams/components/StreamsTable.vue";
import { useStreamPreviews } from "~/features/streams/composables/useStreamPreviews";
import { isTypingTarget, matchShortcut, STREAMS_SHORTCUTS, type StreamsShortcutAction } from "~/features/streams/lib/shortcuts";
import {
	clearOverrides,
	effectiveOperation,
	extractOperation,
	jobLink,
	overridesOf,
	pruneOverrides,
	selectionRows,
	toggleOverride,
	totals
} from "~/features/streams/lib/streams";
import { FilesService } from "~/generated/muxmultitool/files/v1/files_pb";
import { JobsService } from "~/generated/muxmultitool/jobs/v1/jobs_pb";

/**
 * The Streams screen (sketch 02): rules on top that apply to every file of the selection, the table
 * of the active file with a Keep tick per stream and the rule that decided it, the output options,
 * the per-file results, the exact command, and "Queue N jobs". The same screen extracts tracks to
 * one file each (mvp-features.md, "Extract all tracks"): the ticks then say which. The form lives in
 * its store, so it survives a detour to another screen and a preset can be applied into it.
 */
const { t } = useI18n();
const selection = useFileSelectionStore();
const picker = useFilePicker();
const filesClient = useRpcClient(FilesService);
const jobsClient = useRpcClient(JobsService);
const report = useStreamReport(filesClient);

const { form } = storeToRefs(useStreamsFormStore());
const files = computed(() => selection.files);
const extracting = computed(() => form.value.action === "extract");
const { previewOf, fieldsFor, loading } = useStreamPreviews(form, files);

// The active file: the first one until the user picks another; ticks of files that left are dropped
const activeId = ref<string>();
watch(
	() => selection.ids,
	(ids) => {
		if (activeId.value === undefined || !ids.includes(activeId.value)) {
			activeId.value = ids[0];
		}
		form.value.overrides = pruneOverrides(form.value.overrides, ids);
	},
	{ immediate: true }
);
const activeFile = computed(() => files.value.find((file) => file.id === activeId.value));
const activeIndex = computed(() => files.value.findIndex((file) => file.id === activeId.value));
const activePreview = computed(() => (activeId.value === undefined ? undefined : previewOf(activeId.value)));

// Extraction lists the streams from the quick report; a copy gets them from the preview itself
watch(
	[extracting, () => selection.ids],
	([extract, ids]) => {
		if (extract) {
			void report.load(ids);
		}
	},
	{ immediate: true }
);

function rowsOf(fileId: string): TableRow[] {
	const preview = previewOf(fileId);
	const overrides = overridesOf(form.value.overrides, fileId);
	if (!extracting.value) {
		if (!preview?.base) {
			return [];
		}
		const issues = new Map<number, { blocking: boolean; message: string }[]>();
		for (const issue of preview.effective?.issues ?? []) {
			issues.set(issue.streamIndex, [...(issues.get(issue.streamIndex) ?? []), issue]);
		}
		return selectionRows(preview.base, overrides).map((row) => ({ ...row, issues: issues.get(row.stream.index) ?? [] }));
	}
	const outputs = new Map(preview?.extract?.files.map((file) => [file.streamIndex, file.fileName]));
	const skipped = new Map(preview?.extract?.skipped.map((entry) => [entry.streamIndex, entry]));
	return (report.byId(fileId)?.report.streams ?? []).map((stream) => {
		const overridden = overrides.has(stream.index);
		const skip = overridden ? undefined : skipped.get(stream.index);
		return {
			stream,
			kept: !overridden,
			why: skip ? "" : t("streams.table.extracted"),
			overridden,
			output: overridden ? undefined : outputs.get(stream.index),
			issues: skip ? [skip] : []
		};
	});
}
const activeRows = computed(() => (activeId.value === undefined ? [] : rowsOf(activeId.value)));
const activeHasOverrides = computed(() => activeId.value !== undefined && overridesOf(form.value.overrides, activeId.value).size > 0);
const activeCommand = computed(() => activePreview.value?.effective?.command ?? activePreview.value?.extract?.command ?? "");
const activeContainer = computed(() => activePreview.value?.effective?.container);

function toggle(index: number) {
	if (activeId.value !== undefined) {
		form.value.overrides = toggleOverride(form.value.overrides, activeId.value, index);
	}
}
function reset() {
	if (activeId.value !== undefined) {
		form.value.overrides = clearOverrides(form.value.overrides, activeId.value);
	}
}

const summaries = computed<FileSummary[]>(() =>
	files.value.map((file) => {
		const preview = previewOf(file.id);
		const rows = rowsOf(file.id);
		const sums = totals(rows);
		const effective = preview?.effective;
		const extract = preview?.extract;
		const canRun = extracting.value ? extract?.canRun : effective?.canRun;
		const problems = extracting.value
			? (extract?.skipped.filter((entry) => !entry.blocking).length ?? 0)
			: (effective?.issues.filter((issue) => !issue.blocking).length ?? 0) + (effective?.warnings.length ?? 0);
		const output = extracting.value
			? extract
				? t("streams.files.extractFiles", { n: extract.files.length }, extract.files.length)
				: undefined
			: effective?.outputName;
		return {
			id: file.id,
			name: file.name,
			kept: rows.length > 0 ? sums.kept : undefined,
			total: rows.length > 0 ? sums.total : undefined,
			droppedBytes: extracting.value ? undefined : sums.droppedBytes,
			output,
			state: preview?.error ? "error" : !preview || preview.loading ? "loading" : canRun ? "ready" : "blocked",
			problems
		};
	})
);
const blockedCount = computed(() => summaries.value.filter((file) => file.state === "blocked" || file.state === "error").length);
const startable = computed(() => files.value.length > 0 && !loading.value && summaries.value.every((file) => file.state === "ready"));

// Submitting: one job per file, in selection order
const error = ref<AppError>();
const submitting = ref(false);
const submitted = ref<{ id: string; name: string }[]>([]);
async function queue() {
	if (!startable.value || submitting.value) {
		return;
	}
	error.value = undefined;
	submitted.value = [];
	submitting.value = true;
	try {
		for (const file of files.value) {
			const preview = previewOf(file.id);
			const response = await jobsClient.submitJob({
				operation: extracting.value
					? { case: "extractStreams", value: extractOperation(form.value, file.id) }
					: { case: "selectStreams", value: effectiveOperation(form.value, file.id, file.name, preview?.base, fieldsFor(file)) }
			});
			if (response.job) {
				submitted.value = [...submitted.value, { id: response.job.id, name: response.job.name }];
			}
		}
	} catch (cause) {
		error.value = toAppError(cause);
	} finally {
		submitting.value = false;
	}
}

async function addFiles() {
	error.value = undefined;
	try {
		const picked = await picker.pickFiles({ multiple: true });
		selection.add(await entriesForPicked(filesClient, picker.mode, picked));
	} catch (cause) {
		error.value = toAppError(cause);
	}
}

const savePresetOpen = ref(false);

// Keyboard shortcuts, listed in the dialog
const shortcutsOpen = ref(false);
const rulesPanel = useTemplateRef("rulesPanel");
const commandPanel = useTemplateRef("commandPanel");
function selectByOffset(offset: number) {
	const next = files.value[activeIndex.value + offset];
	if (next) {
		activeId.value = next.id;
	}
}
const actions: Record<StreamsShortcutAction, (digit?: number) => void> = {
	queue: () => void queue(),
	previousFile: () => selectByOffset(-1),
	nextFile: () => selectByOffset(1),
	toggleStream: (digit) => {
		if (digit !== undefined && activeRows.value.some((row) => row.stream.index === digit)) {
			toggle(digit);
		}
	},
	toggleVideoOnly: () => {
		if (!extracting.value) {
			form.value.videoOnly = !form.value.videoOnly;
		}
	},
	toggleExtract: () => {
		form.value.action = extracting.value ? "copy" : "extract";
	},
	resetTicks: reset,
	newRule: () => rulesPanel.value?.focusNew(),
	copyCommand: () => commandPanel.value?.copy(),
	showShortcuts: () => {
		shortcutsOpen.value = true;
	}
};
useEventListener(window, "keydown", (event) => {
	if (shortcutsOpen.value || isTypingTarget(event.target)) {
		return;
	}
	const match = matchShortcut(STREAMS_SHORTCUTS, event);
	if (match) {
		event.preventDefault();
		actions[match.action](match.digit);
	}
});

const segment =
	"rounded px-3 py-0.5 text-muted hover:text-text focus-visible:outline-2 focus-visible:outline-accent data-[state=on]:bg-accent data-[state=on]:text-on-accent";
</script>

<template>
	<section class="flex flex-col gap-6 p-6">
		<header class="flex flex-wrap items-start gap-4">
			<div class="min-w-0 flex-1">
				<h2 class="text-xl font-bold">{{ t("nav.streams") }}</h2>
				<p class="mt-1 max-w-prose text-sm text-muted">{{ t("streams.description") }}</p>
			</div>
			<div role="group" :aria-label="t('streams.action.label')">
				<RekaToggleGroupRoot
					type="single"
					:model-value="form.action"
					class="inline-flex rounded-md border border-border p-0.5 text-sm"
					@update:model-value="(value) => (value === 'copy' || value === 'extract') && (form.action = value)"
				>
					<RekaToggleGroupItem value="copy" :class="segment">{{ t("streams.action.copy") }}</RekaToggleGroupItem>
					<RekaToggleGroupItem value="extract" :class="segment">{{ t("streams.action.extract") }}</RekaToggleGroupItem>
				</RekaToggleGroupRoot>
			</div>
			<button
				type="button"
				class="flex items-center gap-1.5 rounded-md border border-border px-3 py-1.5 text-sm hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent"
				@click="savePresetOpen = true"
			>
				<IconStar class="size-4" aria-hidden="true" />
				{{ t("streams.savePreset") }}
			</button>
			<button
				type="button"
				class="rounded-md border border-border p-1.5 hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent"
				:aria-label="t('streams.shortcuts.open')"
				:title="t('streams.shortcuts.open')"
				@click="shortcutsOpen = true"
			>
				<IconKeyboard class="size-4" aria-hidden="true" />
			</button>
		</header>

		<template v-if="files.length === 0">
			<p class="text-sm text-muted">{{ t("streams.empty") }}</p>
			<button
				type="button"
				class="self-start rounded-md border border-border px-3 py-1.5 text-sm hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent"
				@click="addFiles"
			>
				{{ t("streams.files.add") }}
			</button>
		</template>

		<template v-else>
			<StreamsRules
				v-if="!extracting"
				ref="rulesPanel"
				v-model:rules="form.rules"
				v-model:untagged="form.untagged"
				v-model:video-only="form.videoOnly"
				:file-count="files.length"
			/>
			<p v-else class="max-w-prose text-sm text-muted">{{ t("streams.extractNote") }}</p>

			<RekaTabsRoot
				:model-value="activeId"
				orientation="vertical"
				class="grid gap-6 xl:grid-cols-[minmax(0,1fr)_24rem]"
				@update:model-value="activeId = String($event)"
			>
				<div class="flex min-w-0 flex-col gap-3">
					<RekaTabsContent v-if="activeFile" :value="activeFile.id" class="flex min-w-0 flex-col gap-3">
						<h3 class="truncate font-bold" :title="activeFile.name">{{ activeFile.name }}</h3>
						<ErrorPanel v-if="activePreview?.error" :error="activePreview.error" />
						<StreamsTable
							:rows="activeRows"
							:file-name="activeFile.name"
							:loading="activePreview?.loading ?? true"
							:has-overrides="activeHasOverrides"
							:extracting="extracting"
							class="rounded-md border border-border"
							@toggle="toggle"
							@reset="reset"
						/>
						<StreamsNotices :issues="activePreview?.effective?.issues" :warnings="activePreview?.effective?.warnings" />
						<p v-if="activeContainer" class="text-sm text-muted">{{ t("streams.container", { container: activeContainer }) }}</p>
					</RekaTabsContent>
				</div>

				<StreamsFileList :files="summaries" :active-id="activeId" @remove="selection.remove([$event])" @add="addFiles" />
			</RekaTabsRoot>

			<StreamsOutput
				v-model:container="form.container"
				v-model:folder="form.folder"
				v-model:template="form.template"
				:example-name="activeFile?.name"
				:fields="activeFile ? fieldsFor(activeFile) : undefined"
				:extracting="extracting"
			/>

			<StreamsCommand ref="commandPanel" :command="activeCommand" :title="t('streams.command.title', { name: activeFile?.name ?? '' })" />

			<div class="flex flex-wrap items-center gap-3">
				<button
					type="button"
					class="rounded-md bg-accent px-4 py-1.5 text-sm text-on-accent focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-50"
					:disabled="!startable || submitting"
					@click="queue"
				>
					{{ t("streams.queue", { n: files.length }, files.length) }}
				</button>
				<span v-if="loading" class="text-sm text-muted">{{ t("streams.checking") }}</span>
				<span v-else-if="blockedCount > 0" class="text-sm text-danger">{{ t("streams.resolve", { n: blockedCount }, blockedCount) }}</span>
			</div>

			<ErrorPanel v-if="error" :error="error" />
			<p v-if="submitted.length > 0" class="text-sm" role="status">
				{{ t("streams.submitted", { n: submitted.length }, submitted.length) }}
				<template v-for="(job, index) in submitted" :key="job.id">
					<RouterLink :to="jobLink(job.id)" class="text-accent underline">{{ job.name }}</RouterLink
					><template v-if="index < submitted.length - 1">, </template>
				</template>
			</p>
		</template>

		<StreamsShortcutsDialog v-model:open="shortcutsOpen" :shortcuts="STREAMS_SHORTCUTS" prefix="streams.shortcuts" />
		<PresetsSaveDialog v-model:open="savePresetOpen" :form="form" />
	</section>
</template>
