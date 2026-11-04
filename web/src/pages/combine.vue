<script setup lang="ts">
import IconKeyboard from "~icons/lucide/keyboard";

import { useStreamReport } from "~/core/composables/useStreamReport";
import { type AppError, toAppError } from "~/core/libs/errors";
import { entriesForPicked } from "~/core/libs/pickedFiles";
import { move } from "~/core/libs/collections";
import { jobLink } from "~/core/libs/navigation";
import { useRpcClient } from "~/core/libs/rpc";
import { useCombinePreview } from "~/features/streams/composables/useCombinePreview";
import {
	addTrack,
	type CombineForm,
	editSource,
	editTrack,
	emptyCombineForm,
	removeTrack,
	syncSources,
	toCombineOperation
} from "~/features/streams/lib/combine";
import { isTypingTarget, matchShortcut } from "~/core/libs/shortcuts";
import { COMBINE_SHORTCUTS, type CombineShortcutAction } from "~/features/streams/lib/shortcuts";
import { CONTAINERS, renderTemplate } from "~/features/streams/lib/streams";
import { FilesService } from "~/generated/muxmultitool/files/v1/files_pb";
import { JobsService } from "~/generated/muxmultitool/jobs/v1/jobs_pb";

/**
 * The Combine screen (sketch 03): the files of the selection as sources A, B, C, … on the left, the
 * output tracks on the right with their metadata, the backend's warnings (durations that differ,
 * tracks the container cannot hold), the exact command, and "Queue job" with a link to its progress.
 * Covers both MVP cases: subtitles from another file, and video from one source with audio from another.
 */
const { t } = useI18n();
const selection = useFileSelectionStore();
const picker = useFilePicker();
const filesClient = useRpcClient(FilesService);
const jobsClient = useRpcClient(JobsService);
const report = useStreamReport(filesClient);

const form = ref<CombineForm>(emptyCombineForm());
const { preview, checking, error: previewError } = useCombinePreview(form);

// The sources follow the selection; the streams of each come from the quick report
watch(
	() => selection.files,
	(files) => {
		form.value = { ...form.value, ...syncSources(form.value, files) };
		void report.load(files.map((file) => file.id));
	},
	{ immediate: true }
);

const activeTrack = ref<number>();
const outputName = computed(() =>
	form.value.sources[0] ? renderTemplate(form.value.template, form.value.sources[0].name, form.value.container) : ""
);
const blocking = computed(() => preview.value?.issues.filter((issue) => issue.blocking).length ?? 0);
const startable = computed(() => preview.value !== undefined && preview.value.canRun && !checking.value && form.value.tracks.length > 0);

const error = ref<AppError>();
const submitting = ref(false);
const submitted = ref<{ id: string; name: string }>();
async function queue() {
	if (!startable.value || submitting.value) {
		return;
	}
	error.value = undefined;
	submitted.value = undefined;
	submitting.value = true;
	try {
		const response = await jobsClient.submitJob({ operation: { case: "combine", value: toCombineOperation(form.value) } });
		if (response.job) {
			submitted.value = { id: response.job.id, name: response.job.name };
		}
	} catch (cause) {
		error.value = toAppError(cause);
	} finally {
		submitting.value = false;
	}
}

async function addSource() {
	error.value = undefined;
	try {
		const picked = await picker.pickFiles({ multiple: true });
		selection.add(await entriesForPicked(filesClient, picker.mode, picked));
	} catch (cause) {
		error.value = toAppError(cause);
	}
}
function removeSource(position: number) {
	const source = form.value.sources[position];
	if (source) {
		selection.remove([source.id]);
	}
}

function moveActive(offset: number) {
	if (activeTrack.value === undefined) {
		return;
	}
	const to = activeTrack.value + offset;
	if (to >= 0 && to < form.value.tracks.length) {
		form.value.tracks = move(form.value.tracks, activeTrack.value, to);
		activeTrack.value = to;
	}
}
function removeActive() {
	if (activeTrack.value !== undefined) {
		form.value.tracks = removeTrack(form.value.tracks, activeTrack.value);
		activeTrack.value = form.value.tracks.length === 0 ? undefined : Math.min(activeTrack.value, form.value.tracks.length - 1);
	}
}

// Keyboard shortcuts, listed in the dialog
const shortcutsOpen = ref(false);
const commandPanel = useTemplateRef("commandPanel");
const actions: Record<CombineShortcutAction, () => void> = {
	queue: () => void queue(),
	addSource: () => void addSource(),
	trackUp: () => moveActive(-1),
	trackDown: () => moveActive(1),
	removeTrack: removeActive,
	copyCommand: () => commandPanel.value?.copy(),
	showShortcuts: () => {
		shortcutsOpen.value = true;
	}
};
useEventListener(window, "keydown", (event) => {
	if (shortcutsOpen.value || isTypingTarget(event.target)) {
		return;
	}
	const match = matchShortcut(COMBINE_SHORTCUTS, event);
	if (match) {
		event.preventDefault();
		actions[match.action]();
	}
});

const field = "rounded-md border border-border bg-surface px-2 py-1 text-sm";
</script>

<template>
	<section class="flex flex-col gap-6 p-6">
		<header class="flex flex-wrap items-start gap-4">
			<div class="min-w-0 flex-1">
				<h2 class="text-xl font-bold">{{ t("nav.combine") }}</h2>
				<p class="mt-1 max-w-prose text-sm text-muted">{{ t("combine.description") }}</p>
			</div>
			<ShellTooltip :text="t('combine.shortcuts.open')">
				<button
					type="button"
					class="rounded-md border border-border p-1.5 hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent"
					:aria-label="t('combine.shortcuts.open')"
					@click="shortcutsOpen = true"
				>
					<IconKeyboard class="size-4" aria-hidden="true" />
				</button>
			</ShellTooltip>
		</header>

		<div class="grid gap-6 xl:grid-cols-[minmax(0,2fr)_minmax(0,3fr)]">
			<StreamsCombineSources
				:sources="form.sources"
				:tracks="form.tracks"
				:reports="report.cache.value"
				@add="(source, streamIndex) => (form.tracks = addTrack(form.tracks, source, streamIndex))"
				@remove="removeSource"
				@offset="(position, value) => (form.sources = editSource(form.sources, position, { offset: value }))"
				@add-source="addSource"
			/>

			<div class="flex min-w-0 flex-col gap-4">
				<StreamsCombineTracks
					v-model:active="activeTrack"
					:tracks="form.tracks"
					:sources="form.sources"
					:reports="report.cache.value"
					:outputs="preview?.tracks ?? []"
					:issues="preview?.issues ?? []"
					@reorder="form.tracks = $event"
					@remove="form.tracks = removeTrack(form.tracks, $event)"
					@edit="(position, patch) => (form.tracks = editTrack(form.tracks, position, patch))"
				/>
				<ErrorPanel v-if="previewError" :error="previewError" />
				<StreamsNotices :issues="preview?.issues" :warnings="preview?.warnings" />
				<p v-if="preview" class="text-sm text-muted">{{ t("streams.container", { container: preview.container }) }}</p>
			</div>
		</div>

		<section v-if="form.sources.length > 0" class="flex flex-col gap-3" :aria-label="t('streams.output.title')">
			<h3 class="font-bold">{{ t("streams.output.title") }}</h3>
			<div class="flex flex-wrap gap-4">
				<label class="flex flex-col gap-1 text-sm">
					{{ t("streams.output.container") }}
					<select v-model="form.container" :class="field">
						<option v-for="option in CONTAINERS" :key="option.extension" :value="option.extension">{{ option.name }}</option>
					</select>
				</label>
				<label class="flex min-w-64 flex-col gap-1 text-sm">
					{{ t("streams.output.folder") }}
					<input v-model="form.folder" type="text" :class="field" :placeholder="t('combine.output.nextToFirst')" />
					<span class="text-xs text-muted">{{ t("streams.output.folderHint") }}</span>
				</label>
				<label class="flex min-w-64 flex-col gap-1 text-sm">
					{{ t("combine.output.file") }}
					<input v-model="form.template" type="text" :class="field" class="font-mono" />
					<span class="text-xs text-muted">
						{{ t("combine.output.hint") }}
						<template v-if="outputName"
							>→ <span class="font-mono">{{ preview?.outputName || outputName }}</span></template
						>
					</span>
				</label>
			</div>
		</section>

		<StreamsCommand ref="commandPanel" :command="preview?.command ?? ''" :title="t('combine.command')" />

		<div class="flex flex-wrap items-center gap-3">
			<button
				type="button"
				class="rounded-md bg-accent px-4 py-1.5 text-sm text-on-accent focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-50"
				:disabled="!startable || submitting"
				@click="queue"
			>
				{{ t("combine.queue") }}
			</button>
			<span v-if="checking" class="text-sm text-muted">{{ t("streams.checking") }}</span>
			<span v-else-if="form.tracks.length === 0" class="text-sm text-muted">{{ t("combine.needTrack") }}</span>
			<span v-else-if="blocking > 0" class="text-sm text-danger">{{ t("combine.resolve", { n: blocking }, blocking) }}</span>
		</div>

		<ErrorPanel v-if="error" :error="error" />
		<p v-if="submitted" class="text-sm" role="status">
			{{ t("combine.submitted", { name: submitted.name }) }}
			<RouterLink :to="jobLink(submitted.id)" class="text-accent underline">{{ t("combine.viewJob") }}</RouterLink>
		</p>

		<StreamsShortcutsDialog v-model:open="shortcutsOpen" :shortcuts="COMBINE_SHORTCUTS" prefix="combine.shortcuts" />
	</section>
</template>
