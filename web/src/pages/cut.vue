<script setup lang="ts">
import IconKeyboard from "~icons/lucide/keyboard";
import IconPlus from "~icons/lucide/plus";
import IconRedo from "~icons/lucide/redo-2";
import IconScissors from "~icons/lucide/scissors";
import IconTrash from "~icons/lucide/trash";
import IconUndo from "~icons/lucide/undo-2";

import { type AppError, toAppError } from "~/core/libs/errors";
import { entriesForPicked } from "~/core/libs/pickedFiles";
import { useRpcClient } from "~/core/libs/rpc";
import { useCutPlan } from "~/features/cut/composables/useCutPlan";
import { useCutProject } from "~/features/cut/composables/useCutProject";
import { useKeyframes } from "~/features/cut/composables/useKeyframes";
import { useSmartCut } from "~/features/cut/composables/useSmartCut";
import { useThumbnails } from "~/features/cut/composables/useThumbnails";
import { useWaveform } from "~/features/cut/composables/useWaveform";
import { defaultOutput, isDefaultOutput, toCutOperation } from "~/features/cut/lib/export";
import {
	addSegment,
	type EditorState,
	moveSegment,
	removeSegment,
	resizeSegment,
	segmentAt,
	setSegmentTimes,
	sortSegments
} from "~/features/cut/lib/segments";
import { type TimelineAction, timelineShortcutAction } from "~/features/cut/lib/shortcuts";
import { useFileStreams } from "~/features/player/composables/useFileStreams";
import { isTypingTarget } from "~/features/player/lib/shortcuts";
import type { PlayerRange } from "~/features/player/lib/range";
import { CutExport, CutMode, SegmentMode, SourceStatus } from "~/generated/muxmultitool/cut/v1/cut_pb";
import { FilesService, StreamType } from "~/generated/muxmultitool/files/v1/files_pb";
import { JobsService } from "~/generated/muxmultitool/jobs/v1/jobs_pb";

import type CutTimeline from "~/features/cut/components/CutTimeline.vue";
import type PlayerView from "~/features/player/components/PlayerView.vue";

/**
 * Cut screen (sketch 04): the player over the file named by the `file` query (`/cut?file=<id>`), the
 * timeline with thumbnails, waveform, segments and keyframes under it, the segment table and the cut
 * settings beside each other below. Every edit goes to the project on the server (undo/redo there too),
 * and Export submits a cut job.
 */
const { t } = useI18n();
const route = useRoute();
const router = useRouter();
const picker = useFilePicker();
const filesClient = useRpcClient(FilesService);
const jobsClient = useRpcClient(JobsService);

const fileId = computed(() => (typeof route.query.file === "string" && route.query.file !== "" ? route.query.file : undefined));
const player = useTemplateRef<InstanceType<typeof PlayerView>>("player");
const timeline = useTemplateRef<InstanceType<typeof CutTimeline>>("timeline");
const range = ref<PlayerRange>({});
const currentTime = ref(0);
const selectedId = ref<number>();
const visible = ref<{ from: number; to: number; width: number }>();
const audioIndex = ref<number>();
const splitOpen = ref(false);
const shortcutsOpen = ref(false);
const error = ref<AppError>();

// The file: its streams (frame rate, duration, audio tracks) and the analyses behind the lanes
const streams = useFileStreams(fileId);
const keyframes = useKeyframes(fileId, visible, () => streams.duration.value ?? 0, currentTime);
// The stream report knows the length first; the keyframe lookup reports it too, for a file the report cannot read
const duration = computed(() => streams.duration.value || keyframes.duration.value);
const frameRate = computed(() => streams.frameRate.value);
const audioStreams = computed(() =>
	(streams.report.value?.streams ?? [])
		.filter((stream) => stream.type === StreamType.AUDIO)
		.map((stream) => ({ index: stream.index, label: `#${stream.index} ${stream.codec} ${stream.language || "und"}`.trim() }))
);
watch(audioStreams, (list) => {
	if (audioIndex.value === undefined || !list.some((stream) => stream.index === audioIndex.value)) {
		audioIndex.value = list[0]?.index;
	}
});
const thumbnails = useThumbnails(fileId);
const waveform = useWaveform(fileId, audioIndex, () =>
	visible.value ? { from: visible.value.from, to: visible.value.to, peakCount: visible.value.width } : undefined
);

// The project: segments and modes, saved on the server with undo and redo
const project = useCutProject(fileId);
const state = computed(() => project.state.value);
const output = ref("");
watch(
	() => project.sourcePath.value,
	(path) => {
		if (output.value === "" || isDefaultOutput(output.value, path)) {
			output.value = defaultOutput(path, state.value.export);
		}
	}
);
const smart = useSmartCut(fileId);
const plan = useCutPlan(fileId, state, output);
const minLength = computed(() => (frameRate.value ? 1 / frameRate.value : undefined));
const selected = computed(() => state.value.segments.find((segment) => segment.id === selectedId.value));
const statusKey = computed(() => {
	if (project.status.value === "saved" && project.sourceStatus.value === SourceStatus.MISSING) {
		return "missing";
	}
	if (project.status.value === "saved" && project.sourceStatus.value === SourceStatus.CHANGED) {
		return "changed";
	}
	return project.status.value;
});

function commit(patch: Partial<EditorState>): void {
	project.commit({ ...state.value, ...patch });
}
function setSegments(segments: EditorState["segments"]): void {
	commit({ segments });
}
function create(start: number, end: number): void {
	const added = addSegment(state.value.segments, start, end, duration.value);
	if (!added) {
		return;
	}
	setSegments(added.segments);
	selectedId.value = added.id;
}
function remove(id: number): void {
	setSegments(removeSegment(state.value.segments, id));
	if (selectedId.value === id) {
		selectedId.value = undefined;
	}
}
function addFromRange(): void {
	const { start, end } = range.value;
	if (start !== undefined && end !== undefined && end > start) {
		create(start, end);
		range.value = {};
	} else {
		// Without a range, a segment from the playhead to the end of the file: resize it from there
		create(currentTime.value, duration.value);
	}
}
function splitAtPlayhead(): void {
	const at = currentTime.value;
	const target =
		selected.value && selected.value.start < at && at < selected.value.end ? selected.value : segmentAt(state.value.segments, at);
	if (!target || at - target.start < (minLength.value ?? 0) || target.end - at < (minLength.value ?? 0)) {
		return;
	}
	const rest = removeSegment(state.value.segments, target.id);
	const first = addSegment(rest, target.start, at, duration.value);
	const second = first && addSegment(first.segments, at, target.end, duration.value);
	if (!second) {
		return;
	}
	setSegments(sortSegments(second.segments));
	selectedId.value = second.id;
}
function setExport(mode: CutExport): void {
	if (isDefaultOutput(output.value, project.sourcePath.value)) {
		output.value = defaultOutput(project.sourcePath.value, mode);
	}
	commit({ export: mode });
}
function applySplit(parts: ReadonlyArray<{ start: number; end: number }>): void {
	let id = 0;
	commit({
		segments: parts.map((part) => ({ id: ++id, start: part.start, end: part.end })),
		segmentMode: SegmentMode.KEEP,
		cutMode: CutMode.KEYFRAME,
		export: CutExport.SEPARATE_FILES
	});
	setExport(CutExport.SEPARATE_FILES);
	selectedId.value = undefined;
}
function seek(seconds: number): void {
	player.value?.seek(seconds);
	currentTime.value = seconds;
}

function run(action: TimelineAction): void {
	switch (action) {
		case "undo":
			void project.undo();
			break;
		case "redo":
			void project.redo();
			break;
		case "zoomIn":
			timeline.value?.zoomIn();
			break;
		case "zoomOut":
			timeline.value?.zoomOut();
			break;
		case "zoomFit":
			timeline.value?.zoomFit();
			break;
		case "addSegment":
			addFromRange();
			break;
		case "splitAtPlayhead":
			splitAtPlayhead();
			break;
		case "deleteSegment":
			if (selectedId.value !== undefined) {
				remove(selectedId.value);
			}
			break;
	}
}
useEventListener(window, "keydown", (event) => {
	if (!fileId.value || splitOpen.value || shortcutsOpen.value || isTypingTarget(event.target)) {
		return;
	}
	const action = timelineShortcutAction(event);
	if (action) {
		event.preventDefault();
		run(action);
	}
});

// Export: a cut job, linked to its progress
const submitting = ref(false);
const submitted = ref<{ id: string; name: string }>();
async function exportCut(): Promise<void> {
	if (!fileId.value) {
		return;
	}
	error.value = undefined;
	submitted.value = undefined;
	submitting.value = true;
	try {
		const response = await jobsClient.submitJob({
			operation: { case: "cut", value: toCutOperation(fileId.value, state.value, output.value) }
		});
		if (response.job) {
			submitted.value = { id: response.job.id, name: response.job.name };
		}
	} catch (cause) {
		error.value = toAppError(cause);
	} finally {
		submitting.value = false;
	}
}

async function openFile(): Promise<void> {
	error.value = undefined;
	try {
		const [file] = await picker.pickFiles({ extensions: ["mkv", "mp4", "webm", "m2ts", "ts", "mov", "avi"] });
		// The query carries the entry id, the only form the services accept; a file that is gone gives no entry
		const [entry] = file ? await entriesForPicked(filesClient, picker.mode, [file]) : [];
		if (entry) {
			await router.replace({ query: { ...route.query, file: entry.id } });
		}
	} catch (cause) {
		error.value = toAppError(cause);
	}
}
watch(fileId, () => {
	selectedId.value = undefined;
	currentTime.value = 0;
	range.value = {};
	output.value = "";
	submitted.value = undefined;
	error.value = undefined;
});

const toolButton =
	"flex h-8 items-center gap-1.5 rounded-md border border-border px-2 text-sm hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent disabled:cursor-not-allowed disabled:opacity-50";
</script>

<template>
	<section class="flex flex-col gap-4 p-6">
		<header class="flex flex-wrap items-center gap-3">
			<h2 class="text-xl font-bold">{{ t("nav.cut") }}</h2>
			<button
				type="button"
				class="rounded-md bg-accent px-3 py-1.5 text-sm text-on-accent focus-visible:outline-2 focus-visible:outline-accent"
				@click="openFile"
			>
				{{ t("pages.cut.open") }}
			</button>
			<span v-if="fileId" class="truncate font-mono text-xs text-muted" :title="project.sourcePath.value || fileId">
				{{ project.sourcePath.value || fileId }}
			</span>
			<span
				v-if="fileId"
				class="ml-auto text-xs"
				:class="statusKey === 'error' || statusKey === 'missing' ? 'text-danger' : statusKey === 'changed' ? 'text-warning' : 'text-muted'"
				role="status"
			>
				{{ t(`cut.status.${statusKey}`) }}
			</span>
		</header>
		<ErrorPanel v-if="error" :error="error" class="max-w-prose" />
		<ErrorPanel v-if="project.error.value" :error="project.error.value" class="max-w-prose" />

		<template v-if="fileId">
			<PlayerView
				ref="player"
				:key="fileId"
				v-model:range="range"
				:file-id="fileId"
				:keyframes="keyframes.times.value"
				class="max-w-5xl"
				@timeupdate="currentTime = $event"
			/>

			<div class="flex flex-wrap items-center gap-2">
				<button
					type="button"
					:class="toolButton"
					:disabled="!project.canUndo.value"
					:title="t('cut.actions.undoHint')"
					@click="run('undo')"
				>
					<IconUndo class="size-4" aria-hidden="true" />
					{{ t("cut.actions.undo") }}
				</button>
				<button
					type="button"
					:class="toolButton"
					:disabled="!project.canRedo.value"
					:title="t('cut.actions.redoHint')"
					@click="run('redo')"
				>
					<IconRedo class="size-4" aria-hidden="true" />
					{{ t("cut.actions.redo") }}
				</button>
				<span class="mx-1 h-5 w-px bg-border" aria-hidden="true" />
				<button type="button" :class="toolButton" :disabled="!(duration > 0)" :title="t('cut.actions.addHint')" @click="run('addSegment')">
					<IconPlus class="size-4" aria-hidden="true" />
					{{ range.start !== undefined && range.end !== undefined ? t("cut.actions.addRange") : t("cut.actions.add") }}
				</button>
				<button
					type="button"
					:class="toolButton"
					:disabled="!segmentAt(state.segments, currentTime)"
					:title="t('cut.actions.splitHint')"
					@click="run('splitAtPlayhead')"
				>
					<IconScissors class="size-4" aria-hidden="true" />
					{{ t("cut.actions.split") }}
				</button>
				<button type="button" :class="toolButton" :disabled="!selected" :title="t('cut.actions.deleteHint')" @click="run('deleteSegment')">
					<IconTrash class="size-4" aria-hidden="true" />
					{{ t("cut.actions.delete") }}
				</button>
				<ShellIconButton class="ml-auto" :label="t('cut.shortcuts.open')" @click="shortcutsOpen = true">
					<IconKeyboard />
				</ShellIconButton>
			</div>

			<CutTimeline
				ref="timeline"
				v-model:selected-id="selectedId"
				v-model:audio-index="audioIndex"
				:duration="duration"
				:frame-rate="frameRate"
				:current-time="currentTime"
				:segments="state.segments"
				:remove-mode="state.segmentMode === SegmentMode.REMOVE"
				:keyframes="keyframes.times.value"
				:keyframes-loading="keyframes.loading.value"
				:sheet="thumbnails.sheet.value"
				:sprites-ready="thumbnails.spritesReady.value"
				:peaks="waveform.peaks.value"
				:audio-streams="audioStreams"
				@seek="seek"
				@create="create"
				@move="(id, start) => setSegments(moveSegment(state.segments, id, start, duration))"
				@resize="(id, edge, seconds) => setSegments(resizeSegment(state.segments, id, edge, seconds, duration, minLength))"
				@visible="visible = $event"
			/>
			<ErrorPanel v-if="keyframes.error.value" :error="keyframes.error.value" class="max-w-prose" />
			<ErrorPanel v-else-if="thumbnails.error.value" :error="thumbnails.error.value" class="max-w-prose" />
			<ErrorPanel v-else-if="waveform.error.value" :error="waveform.error.value" class="max-w-prose" />

			<div class="grid gap-6 lg:grid-cols-[minmax(0,3fr)_minmax(18rem,2fr)]">
				<section class="flex flex-col gap-2" :aria-label="t('cut.segments.title')">
					<h3 class="font-bold">{{ t("cut.segments.title") }}</h3>
					<CutSegmentTable
						v-model:selected-id="selectedId"
						:segments="state.segments"
						:duration="duration"
						:file-id="fileId"
						@remove="remove"
						@seek="seek"
						@times="(id, start, end) => setSegments(setSegmentTimes(state.segments, id, start, end))"
					/>
				</section>
				<CutSettings
					:state="state"
					:duration="duration"
					:smart="smart.check.value"
					:smart-checking="smart.checking.value"
					:plan="plan.plan.value"
					:planning="plan.planning.value"
					:plan-error="plan.error.value"
					:output="output"
					:submitting="submitting"
					:submitted="submitted"
					@update:segment-mode="commit({ segmentMode: $event })"
					@update:cut-mode="commit({ cutMode: $event })"
					@update:export="setExport"
					@update:output="output = $event"
					@split="splitOpen = true"
					@export="exportCut"
				/>
			</div>

			<CutSplitDialog v-model:open="splitOpen" :file-id="fileId" :output="output" @apply="applySplit" />
			<CutShortcutsDialog v-model:open="shortcutsOpen" />
		</template>
		<template v-else>
			<p class="max-w-prose text-muted">{{ t("pages.cut.description") }}</p>
			<p class="text-sm text-muted">{{ t("pages.cut.empty") }}</p>
		</template>
	</section>
</template>
