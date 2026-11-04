<script setup lang="ts">
import IconCaptions from "~icons/lucide/captions";
import IconKeyboard from "~icons/lucide/keyboard";
import IconVolume from "~icons/lucide/volume-2";
import IconVolumeOff from "~icons/lucide/volume-x";

import { PreviewMode } from "~/generated/muxmultitool/media/v1/media_pb";

import { useFileStreams } from "../composables/useFileStreams";
import { usePreviewSession } from "../composables/usePreviewSession";
import { useSubtitleCues } from "../composables/useSubtitleCues";
import { activeCues, isTextSubtitle } from "../lib/cues";
import { type AttachedSource, attachPreview, detectPlaybackEngine, type PlaybackEngine } from "../lib/engine";
import { crossedEnd, type PlayerRange, rangeBounds, withRangeEnd, withRangeStart } from "../lib/range";
import { isDialogTarget, isTypingTarget, type PlayerAction, SEEK_STEP_SECONDS, shortcutAction } from "../lib/shortcuts";
import { clampTime, nextKeyframe, previousKeyframe, stepFrame } from "../lib/stepping";
import { frameIndex, frameStart } from "../lib/timecode";

/**
 * The preview player: the file's HLS preview in a <video> (hls.js on MediaSource, per the TASK-4 spike),
 * frame and keyframe stepping, a playable or loopable range, subtitle cues over the picture, and the
 * fixed keyboard shortcuts.
 *
 * Keyframe times come from the host (the cut service's keyframe lookup); without them the keyframe
 * buttons are disabled. The frame rate comes from the stream report unless the host knows better.
 */
const {
	fileId,
	keyframes = [],
	frameRate: frameRateOverride
} = defineProps<{
	/** The file, as FilesService names it (or, until it issues ids, its absolute path in a media root). */
	fileId: string;
	/** Keyframe times in seconds, ascending. */
	keyframes?: readonly number[];
	/** Overrides the frame rate from the stream report. */
	frameRate?: number;
}>();
const range = defineModel<PlayerRange>("range", { default: () => ({}) });
const emit = defineEmits<{ timeupdate: [seconds: number] }>();
const { t } = useI18n();

const video = useTemplateRef<HTMLVideoElement>("video");
const preview = usePreviewSession(() => fileId);
const streams = useFileStreams(() => fileId);
const frameRate = computed(() => frameRateOverride ?? streams.frameRate.value);
const duration = computed(() => preview.session.value?.durationSeconds || streams.duration.value || 0);
/**
 * The time the transport shows and the range takes: the start of the frame on screen when the frame rate
 * is known (a frame step seeks a hair past the frame start so the browser lands on it), the raw playhead
 * otherwise.
 */
const displayTime = computed(() => {
	const fps = frameRate.value;
	return fps ? frameStart(frameIndex(currentTime.value, fps), fps) : currentTime.value;
});

// Playback state, mirrored from the element so the template never reads it directly
const engine = ref<PlaybackEngine>();
const engineVersion = ref<string>();
const fatal = ref<string>();
const currentTime = ref(0);
const paused = ref(true);
const muted = ref(false);
const loop = ref(false);
const playingRange = ref(false);
const shortcutsOpen = ref(false);

const modeKey = computed(() => {
	switch (preview.session.value?.mode) {
		case PreviewMode.REMUX:
			return "remux";
		case PreviewMode.TRANSCODE_AUDIO:
			return "transcodeAudio";
		case PreviewMode.TRANSCODE:
			return "transcode";
		default:
			return "unspecified";
	}
});

// Source attachment follows the session; the engine is chosen once the element exists
let attached: AttachedSource | undefined;
function detach(): void {
	attached?.detach();
	attached = undefined;
}
watch(
	[video, () => preview.session.value],
	async ([element, session]) => {
		detach();
		fatal.value = undefined;
		if (!element || !session) {
			return;
		}
		const chosen = detectPlaybackEngine(element);
		engine.value = chosen;
		const source = await attachPreview(element, session.playlistUrl, chosen, {
			onFatalError: (detail) => {
				fatal.value = detail;
			}
		});
		if (preview.session.value !== session || video.value !== element) {
			source.detach();
			return;
		}
		attached = source;
		engineVersion.value = source.version;
	},
	{ immediate: true }
);
onBeforeUnmount(detach);

// The playhead: read every animation frame while playing, and on the element's own events otherwise
let lastTime = 0;
function sync(): void {
	const element = video.value;
	if (!element) {
		return;
	}
	const now = element.currentTime;
	const { start, end } = rangeBounds(range.value, duration.value);
	if (!element.paused && (loop.value || playingRange.value) && crossedEnd(lastTime, now, end)) {
		if (loop.value) {
			element.currentTime = start;
		} else {
			element.pause();
			element.currentTime = end;
			playingRange.value = false;
		}
	}
	lastTime = element.currentTime;
	if (currentTime.value !== lastTime) {
		currentTime.value = lastTime;
		emit("timeupdate", lastTime);
	}
}
const ticker = useRafFn(sync, { immediate: false });
function onPlay(): void {
	paused.value = false;
	ticker.resume();
}
function onPause(): void {
	paused.value = true;
	ticker.pause();
	sync();
}
function onEnded(): void {
	onPause();
	if (loop.value) {
		seek(rangeBounds(range.value, duration.value).start);
		void play();
	}
}
function onVolumeChange(): void {
	muted.value = video.value?.muted ?? false;
}

// Controls
function seek(seconds: number): void {
	const element = video.value;
	if (!element) {
		return;
	}
	const target = clampTime(seconds, duration.value);
	element.currentTime = target;
	lastTime = target;
	currentTime.value = target;
	emit("timeupdate", target);
}
async function play(): Promise<void> {
	try {
		await video.value?.play();
	} catch {
		// Autoplay refused or the source went away; the element stays paused and the button shows it
	}
}
function pause(): void {
	video.value?.pause();
	playingRange.value = false;
}
function stepFrames(delta: number): void {
	const element = video.value;
	if (!element) {
		return;
	}
	pause();
	seek(stepFrame(element.currentTime, frameRate.value ?? 25, delta, duration.value));
}
function stepKeyframes(direction: -1 | 1): void {
	const element = video.value;
	if (!element || keyframes.length === 0) {
		return;
	}
	const target = direction > 0 ? nextKeyframe(keyframes, element.currentTime) : previousKeyframe(keyframes, element.currentTime);
	if (target !== undefined) {
		pause();
		seek(target);
	}
}
function playRange(): void {
	const { start } = rangeBounds(range.value, duration.value);
	seek(start);
	playingRange.value = true;
	void play();
}
function run(action: PlayerAction): void {
	switch (action) {
		case "togglePlay":
			if (paused.value) {
				void play();
			} else {
				pause();
			}
			break;
		case "frameBack":
			stepFrames(-1);
			break;
		case "frameForward":
			stepFrames(1);
			break;
		case "keyframeBack":
			stepKeyframes(-1);
			break;
		case "keyframeForward":
			stepKeyframes(1);
			break;
		case "seekBack":
			seek(currentTime.value - SEEK_STEP_SECONDS);
			break;
		case "seekForward":
			seek(currentTime.value + SEEK_STEP_SECONDS);
			break;
		case "seekStart":
			seek(0);
			break;
		case "seekEnd":
			pause();
			seek(duration.value);
			break;
		case "setRangeStart":
			range.value = withRangeStart(range.value, displayTime.value);
			break;
		case "setRangeEnd":
			range.value = withRangeEnd(range.value, displayTime.value);
			break;
		case "playRange":
			playRange();
			break;
		case "toggleLoop":
			loop.value = !loop.value;
			break;
		case "toggleMute":
			if (video.value) {
				video.value.muted = !video.value.muted;
			}
			break;
		case "showShortcuts":
			shortcutsOpen.value = true;
			break;
	}
}
useEventListener(window, "keydown", (event) => {
	if (shortcutsOpen.value || isTypingTarget(event.target) || isDialogTarget(event.target)) {
		return;
	}
	const action = shortcutAction(event);
	if (action) {
		event.preventDefault();
		run(action);
	}
});

// Subtitles
const subtitleIndex = ref<number>();
const subtitles = useSubtitleCues(
	() => fileId,
	() => subtitleIndex.value
);
const shownCues = computed(() => activeCues(subtitles.cues.value, currentTime.value));
function trackLabel(index: number, language: string, codec: string): string {
	const base = t("player.subtitles.track", { index, language: language || "und", codec });
	return isTextSubtitle(codec) ? base : `${base} · ${t("player.subtitles.image")}`;
}
function selectTrack(event: Event): void {
	const value = (event.target as HTMLSelectElement).value;
	subtitleIndex.value = value === "" ? undefined : Number(value);
}
watch(
	() => fileId,
	() => {
		subtitleIndex.value = undefined;
		range.value = {};
		loop.value = false;
		playingRange.value = false;
		currentTime.value = 0;
		lastTime = 0;
	}
);

defineExpose({ seek, play, pause, stepFrames, stepKeyframes, run, currentTime: readonly(currentTime), paused: readonly(paused) });
</script>

<template>
	<section class="flex flex-col gap-2">
		<header class="flex flex-wrap items-center gap-2 text-sm">
			<span class="text-muted">{{ t(`player.mode.${modeKey}`) }}</span>
			<span v-if="engine" class="text-xs text-muted">
				· {{ t("player.engine.label", { engine: engineVersion ? `${engine} ${engineVersion}` : engine }) }}
			</span>
			<span v-if="frameRate" class="text-xs text-muted">· {{ t("player.transport.fps", { fps: frameRate.toFixed(3) }) }}</span>

			<div class="ml-auto flex items-center gap-1">
				<label class="flex items-center gap-1.5">
					<IconCaptions class="size-4 text-muted" aria-hidden="true" />
					<span class="sr-only">{{ t("player.subtitles.label") }}</span>
					<select
						class="h-8 rounded-md border border-border bg-surface-sunken px-1.5 text-sm focus-visible:outline-2 focus-visible:outline-accent"
						:value="subtitleIndex ?? ''"
						@change="selectTrack"
					>
						<option value="">{{ t("player.subtitles.off") }}</option>
						<option
							v-for="stream in streams.subtitles.value"
							:key="stream.index"
							:value="stream.index"
							:disabled="!isTextSubtitle(stream.codec)"
						>
							{{ trackLabel(stream.index, stream.language, stream.codec) }}
						</option>
					</select>
				</label>
				<span v-if="subtitles.status.value === 'loading'" class="text-xs text-muted">{{ t("player.subtitles.loading") }}</span>
				<ShellIconButton :label="muted ? t('player.unmute') : t('player.mute')" @click="run('toggleMute')">
					<IconVolumeOff v-if="muted" />
					<IconVolume v-else />
				</ShellIconButton>
				<ShellIconButton :label="t('player.shortcuts.open')" @click="shortcutsOpen = true">
					<IconKeyboard />
				</ShellIconButton>
			</div>
		</header>

		<div class="stage relative aspect-video w-full overflow-hidden rounded-md @container">
			<video
				ref="video"
				class="size-full"
				playsinline
				preload="auto"
				@click="run('togglePlay')"
				@play="onPlay"
				@pause="onPause"
				@ended="onEnded"
				@seeked="sync"
				@timeupdate="sync"
				@loadedmetadata="sync"
				@volumechange="onVolumeChange"
			/>
			<PlayerSubtitleOverlay :cues="shownCues" />
			<!-- A host may draw over the picture, e.g. the crop overlay of the Images screen -->
			<slot name="overlay" />
			<div
				v-if="preview.status.value === 'starting' || engine === 'none' || fatal"
				class="notice absolute inset-0 flex items-center justify-center p-6 text-center text-sm"
				:class="{ 'notice-opaque': engine === 'none' || fatal }"
			>
				<p v-if="engine === 'none'" class="max-w-prose">{{ t("player.engine.none") }}</p>
				<p v-else-if="fatal" class="max-w-prose">{{ t("player.fatal", { detail: fatal }) }}</p>
				<p v-else>{{ t("player.starting") }}</p>
			</div>
		</div>

		<ErrorPanel v-if="preview.error.value" :error="preview.error.value" />
		<ErrorPanel v-else-if="subtitles.error.value" :error="subtitles.error.value" />
		<ErrorPanel v-else-if="streams.error.value" :error="streams.error.value" />

		<PlayerTransport
			:paused="paused"
			:current-time="displayTime"
			:duration="duration"
			:frame-rate="frameRate"
			:has-keyframes="keyframes.length > 0"
			:range="range"
			:loop="loop"
			:playing-range="playingRange"
			@action="run"
			@seek="seek"
			@clear-range="range = {}"
		/>

		<PlayerShortcutsDialog v-model:open="shortcutsOpen" />
	</section>
</template>

<style scoped>
/* The stage is picture, not chrome: letterboxing is black and a notice over it reads white in every theme */
.stage {
	background: #000;
}
.notice {
	color: #fff;
	background: rgb(0 0 0 / 0.4);
}
.notice-opaque {
	background: rgb(0 0 0 / 0.8);
}
</style>
