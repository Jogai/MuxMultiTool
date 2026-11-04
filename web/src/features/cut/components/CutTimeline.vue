<script setup lang="ts">
import IconMagnet from "~icons/lucide/magnet";
import IconZoomIn from "~icons/lucide/zoom-in";
import IconZoomOut from "~icons/lucide/zoom-out";

import { formatTimecode } from "~/core/libs/format";
import type { ThumbnailSheet } from "~/generated/muxmultitool/media/v1/media_pb";

import type { WaveformPeaks } from "../composables/useWaveform";
import { DEFAULT_MIN_LENGTH, type Segment, snapToKeyframe } from "../lib/segments";
import { nudgeSegment, segmentShortcutAction } from "../lib/shortcuts";
import { spriteUrl, thumbnailSlots } from "../lib/thumbnails";
import {
	fitView,
	pxToSeconds,
	resizeView,
	scrollByPx,
	scrollIntoView,
	scrollTo,
	type TimelineView,
	tickLabel,
	tickStep,
	ticks,
	timeToX,
	visibleRange,
	visibleSeconds,
	withDuration,
	xToTime,
	zoomAtFraction,
	zoomBy,
	zoomFraction
} from "../lib/view";

/**
 * The timeline of the cut editor (sketch 04): a ruler, the thumbnails, the waveform of one audio
 * stream, the segments and the keyframes, over a zoomable, scrollable view of the file. Segments are
 * created by dragging on the segment lane, moved by their body and resized by their edges, with the
 * edges snapping to keyframes and the playhead (hold Alt to drag freely). Clicking any other lane seeks.
 * The segments take the focus too (one tab stop, the selected one): the arrow keys nudge the focused
 * segment or one of its edges by a frame, Up and Down move to a neighbour (`lib/shortcuts`).
 *
 * The data comes from the host: the keyframes and waveform for the visible window (`visible` tells it
 * what that is), the thumbnail sheet, the segments. Edits are emitted, not applied: the host commits
 * them to the project.
 */
const {
	duration,
	frameRate,
	currentTime,
	segments,
	selectedId,
	removeMode = false,
	keyframes = [],
	keyframesLoading = false,
	sheet,
	spritesReady = 0,
	peaks,
	audioStreams = [],
	audioIndex
} = defineProps<{
	duration: number;
	frameRate?: number;
	currentTime: number;
	segments: readonly Segment[];
	selectedId?: number;
	/** Segments are the parts cut away (REMOVE mode): drawn hatched instead of filled. */
	removeMode?: boolean;
	keyframes?: readonly number[];
	keyframesLoading?: boolean;
	sheet?: ThumbnailSheet;
	spritesReady?: number;
	peaks?: WaveformPeaks;
	/** The audio streams the waveform can come from. */
	audioStreams?: ReadonlyArray<{ index: number; label: string }>;
	audioIndex?: number;
}>();
const emit = defineEmits<{
	seek: [seconds: number];
	"update:selectedId": [id: number | undefined];
	"update:audioIndex": [index: number | undefined];
	create: [start: number, end: number];
	move: [id: number, start: number];
	resize: [id: number, edge: "start" | "end", seconds: number];
	visible: [window: { from: number; to: number; width: number }];
}>();
const { t } = useI18n();

const LANES = { ruler: 24, thumbs: 56, wave: 48, segments: 40, keyframes: 14 } as const;
const SNAP_PX = 8;
const CLICK_PX = 3;

const lanes = useTemplateRef<HTMLDivElement>("lanes");
const waveCanvas = useTemplateRef<HTMLCanvasElement>("waveCanvas");
const keyframeCanvas = useTemplateRef<HTMLCanvasElement>("keyframeCanvas");
const { width } = useElementSize(lanes);
const view = ref<TimelineView>(fitView(0, 0));
const snapping = ref(true);

// The view follows the file and the viewport; zoom and scroll are the user's
watch(
	() => duration,
	(length) => {
		view.value = withDuration(view.value, length);
	}
);
watch(width, (w) => {
	view.value = resizeView(view.value, w, frameRate);
});
watch(
	view,
	(current) => {
		if (current.width > 0 && current.duration > 0) {
			const { from, to } = visibleRange(current);
			emit("visible", { from, to, width: Math.round(current.width) });
		}
	},
	{ immediate: true }
);
watch(
	() => currentTime,
	(seconds) => {
		if (!drag.value) {
			view.value = scrollIntoView(view.value, seconds);
		}
	}
);

// Zoom: buttons and slider around the playhead when it is visible, the wheel around the pointer
const zoomAnchor = computed(() => {
	const x = timeToX(view.value, currentTime);
	return x >= 0 && x <= view.value.width ? x : view.value.width / 2;
});
const zoomSlider = computed(() => zoomFraction(view.value, frameRate));
function zoomIn(): void {
	view.value = zoomBy(view.value, 1.5, zoomAnchor.value, frameRate);
}
function zoomOut(): void {
	view.value = zoomBy(view.value, 1 / 1.5, zoomAnchor.value, frameRate);
}
function zoomFit(): void {
	view.value = fitView(duration, view.value.width);
}
function setZoomFraction(event: Event): void {
	const fraction = Number((event.target as HTMLInputElement).value);
	view.value = zoomBy(view.value, zoomAtFraction(view.value, fraction, frameRate) / view.value.pxPerSecond, zoomAnchor.value, frameRate);
}
/** Zooms so the segment fills most of the viewport. */
function zoomToSegment(segment: Segment): void {
	const length = Math.max(segment.end - segment.start, DEFAULT_MIN_LENGTH);
	const zoomed = zoomBy(view.value, (view.value.width * 0.8) / length / view.value.pxPerSecond, 0, frameRate);
	view.value = scrollTo(zoomed, segment.start - visibleSeconds(zoomed) * 0.1);
}
function onWheel(event: WheelEvent): void {
	const rect = lanes.value?.getBoundingClientRect();
	if (!rect) {
		return;
	}
	if (event.ctrlKey || event.metaKey) {
		view.value = zoomBy(view.value, event.deltaY < 0 ? 1.25 : 0.8, event.clientX - rect.left, frameRate);
	} else {
		const delta = Math.abs(event.deltaX) > Math.abs(event.deltaY) ? event.deltaX : event.deltaY;
		view.value = scrollByPx(view.value, delta);
	}
}

// Pointer interaction: one drag at a time, captured on the lanes element
interface Drag {
	kind: "seek" | "create" | "move" | "resize" | "scrollbar";
	pointerId: number;
	startX: number;
	anchor: number;
	segment?: Segment;
	edge?: "start" | "end";
	moved: boolean;
}
const drag = ref<Drag>();
/** The segment being dragged, with its draft times, drawn in place of the real one. */
const preview = ref<Segment>();
const minLength = computed(() => (frameRate ? 1 / frameRate : DEFAULT_MIN_LENGTH));

function pointerX(event: PointerEvent): number {
	const rect = lanes.value?.getBoundingClientRect();
	return rect ? event.clientX - rect.left : 0;
}
function snap(seconds: number, free: boolean): number {
	if (!snapping.value || free) {
		return seconds;
	}
	const threshold = pxToSeconds(view.value, SNAP_PX);
	const toKeyframe = snapToKeyframe(seconds, keyframes, threshold);
	// The playhead is a snap target too, so a cut can be set exactly where the picture stands
	return Math.abs(currentTime - seconds) <= threshold && Math.abs(currentTime - seconds) < Math.abs(toKeyframe - seconds)
		? currentTime
		: toKeyframe;
}
type DragStart = Omit<Drag, "pointerId" | "startX" | "moved">;
/** Starts a drag from a pointer down on one of the lanes; the pointer is captured on the lanes element. */
function begin(event: PointerEvent, make: (seconds: number) => DragStart | undefined): void {
	if (event.button !== 0 || !(duration > 0) || !lanes.value) {
		return;
	}
	const x = pointerX(event);
	const started = make(xToTime(view.value, x));
	if (!started) {
		return;
	}
	drag.value = { ...started, pointerId: event.pointerId, startX: x, moved: false };
	lanes.value.setPointerCapture(event.pointerId);
	event.preventDefault();
	event.stopPropagation();
}
function startSeek(event: PointerEvent): void {
	begin(event, (seconds) => {
		emit("seek", seconds);
		return { kind: "seek", anchor: seconds };
	});
}
function startCreate(event: PointerEvent): void {
	begin(event, (seconds) => ({ kind: "create", anchor: snap(seconds, event.altKey) }));
}
function startSegment(event: PointerEvent, segment: Segment, edge?: "start" | "end"): void {
	begin(event, (seconds) => {
		emit("update:selectedId", segment.id);
		return edge ? { kind: "resize", anchor: seconds, segment, edge } : { kind: "move", anchor: seconds - segment.start, segment };
	});
}
function startScrollbar(event: PointerEvent): void {
	begin(event, () => ({ kind: "scrollbar", anchor: view.value.scroll }));
}
function onPointerMove(event: PointerEvent): void {
	const current = drag.value;
	if (!current || current.pointerId !== event.pointerId) {
		return;
	}
	const x = pointerX(event);
	if (!current.moved && Math.abs(x - current.startX) < CLICK_PX) {
		return;
	}
	current.moved = true;
	const seconds = xToTime(view.value, x);
	switch (current.kind) {
		case "seek":
			emit("seek", seconds);
			break;
		case "scrollbar": {
			const ratio = duration / Math.max(view.value.width, 1);
			view.value = scrollTo(view.value, current.anchor + (x - current.startX) * ratio);
			break;
		}
		case "create": {
			const end = snap(seconds, event.altKey);
			preview.value = { id: 0, start: Math.min(current.anchor, end), end: Math.max(current.anchor, end) };
			break;
		}
		case "move": {
			const segment = current.segment!;
			const length = segment.end - segment.start;
			let start = seconds - current.anchor;
			// Whichever edge is nearer a snap target wins
			const snappedStart = snap(start, event.altKey);
			const snappedEnd = snap(start + length, event.altKey) - length;
			start = Math.abs(snappedStart - start) <= Math.abs(snappedEnd - start) ? snappedStart : snappedEnd;
			start = Math.min(Math.max(start, 0), Math.max(0, duration - length));
			preview.value = { ...segment, start, end: start + length };
			break;
		}
		case "resize": {
			const segment = current.segment!;
			const snapped = snap(seconds, event.altKey);
			preview.value =
				current.edge === "start"
					? { ...segment, start: Math.min(Math.max(snapped, 0), segment.end - minLength.value) }
					: { ...segment, end: Math.max(Math.min(snapped, duration), segment.start + minLength.value) };
			break;
		}
	}
}
function onPointerUp(event: PointerEvent): void {
	const current = drag.value;
	if (!current || current.pointerId !== event.pointerId) {
		return;
	}
	const draft = preview.value;
	drag.value = undefined;
	preview.value = undefined;
	lanes.value?.releasePointerCapture(event.pointerId);
	switch (current.kind) {
		case "create":
			if (current.moved && draft && draft.end - draft.start >= minLength.value) {
				emit("create", draft.start, draft.end);
			} else if (!current.moved) {
				emit("update:selectedId", undefined);
				emit("seek", current.anchor);
			}
			break;
		case "move":
			if (current.moved && draft) {
				emit("move", draft.id, draft.start);
			}
			break;
		case "resize":
			if (current.moved && draft) {
				emit("resize", draft.id, current.edge!, current.edge === "start" ? draft.start : draft.end);
			}
			break;
		default:
			break;
	}
}

// Keyboard: the selected segment is the lane's tab stop (the first shown one when none is selected or it is scrolled away)
const segmentLane = useTemplateRef<HTMLDivElement>("segmentLane");
const focusId = computed(() =>
	shown.value.some((segment) => segment.id === selectedId) ? selectedId : shown.value.find((segment) => segment.id !== 0)?.id
);
function segmentName(segment: Segment & { index: number }): string {
	return t("cut.timeline.segmentName", { n: segment.index, start: formatTimecode(segment.start), end: formatTimecode(segment.end) });
}
function onSegmentKey(event: KeyboardEvent, segment: Segment): void {
	const action = segmentShortcutAction(event);
	if (!action) {
		return;
	}
	// The player steps frames on the same keys; a focused segment takes them for itself
	event.preventDefault();
	event.stopPropagation();
	if (action === "previous" || action === "next") {
		const options = [...(segmentLane.value?.querySelectorAll<HTMLElement>("[role=option]") ?? [])];
		const index = options.indexOf(event.currentTarget as HTMLElement);
		options[index + (action === "next" ? 1 : -1)]?.focus();
		return;
	}
	const nudge = nudgeSegment(segment, action, minLength.value, duration, minLength.value);
	if (nudge.kind === "move") {
		emit("move", segment.id, nudge.start);
	} else {
		emit("resize", segment.id, nudge.edge, nudge.seconds);
	}
}

// Lane contents
const step = computed(() => tickStep(view.value.pxPerSecond, 90, frameRate));
const rulerTicks = computed(() => ticks(view.value, 90, frameRate));
const slots = computed(() => (sheet ? thumbnailSlots(view.value, sheet, spritesReady, LANES.thumbs) : []));
const thumbScale = computed(() => (sheet && sheet.tileHeight > 0 ? LANES.thumbs / sheet.tileHeight : 1));
const shown = computed(() =>
	segments
		.map((segment) => (preview.value && preview.value.id === segment.id ? preview.value : segment))
		.concat(preview.value && preview.value.id === 0 ? [preview.value] : [])
		.map((segment, index) => ({
			...segment,
			index: index + 1,
			x: timeToX(view.value, segment.start),
			w: (segment.end - segment.start) * view.value.pxPerSecond
		}))
		.filter((segment) => segment.x + segment.w >= 0 && segment.x <= view.value.width)
);
const playheadX = computed(() => timeToX(view.value, currentTime));
const scrollbar = computed(() => {
	const total = Math.max(duration, 1e-9);
	return { left: (view.value.scroll / total) * 100, width: Math.min(100, (visibleSeconds(view.value) / total) * 100) };
});
const waveformNote = computed(() => {
	if (audioIndex === undefined) {
		return audioStreams.length === 0 ? t("cut.timeline.noAudio") : undefined;
	}
	if (peaks && !peaks.done && peaks.analyzedSeconds < peaks.to) {
		return t("cut.timeline.analysing", { time: formatTimecode(peaks.analyzedSeconds) });
	}
	return undefined;
});

function cssColor(name: string, fallback: string): string {
	const value = lanes.value ? getComputedStyle(lanes.value).getPropertyValue(name).trim() : "";
	return value || fallback;
}
function drawWaveform(): void {
	const canvas = waveCanvas.value;
	if (!canvas) {
		return;
	}
	const dpr = globalThis.devicePixelRatio || 1;
	const w = Math.max(1, Math.round(view.value.width));
	canvas.width = Math.round(w * dpr);
	canvas.height = Math.round(LANES.wave * dpr);
	const ctx = canvas.getContext("2d");
	if (!ctx) {
		return;
	}
	ctx.scale(dpr, dpr);
	ctx.clearRect(0, 0, w, LANES.wave);
	if (!peaks || peaks.received === 0) {
		return;
	}
	ctx.fillStyle = cssColor("--mmt-accent", "#2563eb");
	ctx.globalAlpha = 0.75;
	const mid = LANES.wave / 2;
	const half = LANES.wave / 2 - 1;
	const span = peaks.to - peaks.from;
	for (let x = 0; x < w; x += 1) {
		const seconds = xToTime(view.value, x + 0.5);
		if (seconds < peaks.from || seconds >= peaks.to) {
			continue;
		}
		const index = Math.min(peaks.count - 1, Math.floor(((seconds - peaks.from) / span) * peaks.count));
		if (index >= peaks.received) {
			continue;
		}
		const top = mid - (peaks.maxs[index] ?? 0) * half;
		const bottom = mid - (peaks.mins[index] ?? 0) * half;
		ctx.fillRect(x, top, 1, Math.max(1, bottom - top));
	}
}
function drawKeyframes(): void {
	const canvas = keyframeCanvas.value;
	if (!canvas) {
		return;
	}
	const dpr = globalThis.devicePixelRatio || 1;
	const w = Math.max(1, Math.round(view.value.width));
	canvas.width = Math.round(w * dpr);
	canvas.height = Math.round(LANES.keyframes * dpr);
	const ctx = canvas.getContext("2d");
	if (!ctx) {
		return;
	}
	ctx.scale(dpr, dpr);
	ctx.clearRect(0, 0, w, LANES.keyframes);
	ctx.fillStyle = cssColor("--mmt-muted", "#5f6672");
	const { from, to } = visibleRange(view.value);
	for (const keyframe of keyframes) {
		if (keyframe < from || keyframe > to) {
			continue;
		}
		ctx.fillRect(Math.round(timeToX(view.value, keyframe)), 2, 1, LANES.keyframes - 4);
	}
}
watch([view, () => peaks, waveCanvas], () => nextTick(drawWaveform), { flush: "post" });
watch([view, () => keyframes, keyframeCanvas], () => nextTick(drawKeyframes), { flush: "post" });

function selectAudio(event: Event): void {
	const value = (event.target as HTMLSelectElement).value;
	emit("update:audioIndex", value === "" ? undefined : Number(value));
}

defineExpose({ zoomIn, zoomOut, zoomFit, zoomToSegment });
</script>

<template>
	<section class="flex flex-col gap-1 rounded-md border border-border bg-surface select-none" :aria-label="t('cut.timeline.title')">
		<header class="flex flex-wrap items-center gap-3 px-2 pt-1.5 text-xs text-muted">
			<div class="flex items-center gap-1" role="group" :aria-label="t('cut.timeline.zoom')">
				<span>{{ t("cut.timeline.zoom") }}</span>
				<ShellIconButton :label="t('cut.timeline.zoomOut')" @click="zoomOut">
					<IconZoomOut class="size-4" />
				</ShellIconButton>
				<input
					type="range"
					class="w-28 accent-(--mmt-accent)"
					min="0"
					max="1"
					step="0.01"
					:value="zoomSlider"
					:aria-label="t('cut.timeline.zoom')"
					@input="setZoomFraction"
				/>
				<ShellIconButton :label="t('cut.timeline.zoomIn')" @click="zoomIn">
					<IconZoomIn class="size-4" />
				</ShellIconButton>
				<button type="button" class="rounded px-1.5 py-0.5 hover:bg-surface-sunken hover:text-text" @click="zoomFit">
					{{ t("cut.timeline.fit") }}
				</button>
			</div>
			<button
				type="button"
				class="flex items-center gap-1 rounded px-1.5 py-0.5 hover:bg-surface-sunken hover:text-text aria-pressed:text-accent"
				:aria-pressed="snapping"
				:title="t('cut.timeline.snapHint')"
				@click="snapping = !snapping"
			>
				<IconMagnet class="size-3.5" aria-hidden="true" />
				{{ t("cut.timeline.snap") }}
			</button>
			<label class="flex items-center gap-1">
				{{ t("cut.timeline.waveformFrom") }}
				<select
					class="h-7 rounded-md border border-border bg-surface-sunken px-1 text-xs text-text focus-visible:outline-2 focus-visible:outline-accent"
					:value="audioIndex ?? ''"
					:disabled="audioStreams.length === 0"
					@change="selectAudio"
				>
					<option value="">{{ t("cut.timeline.waveformOff") }}</option>
					<option v-for="stream in audioStreams" :key="stream.index" :value="stream.index">{{ stream.label }}</option>
				</select>
			</label>
			<span v-if="waveformNote">{{ waveformNote }}</span>
			<span v-if="keyframesLoading" class="ml-auto">{{ t("cut.timeline.keyframesLoading") }}</span>
		</header>

		<div class="flex">
			<div class="flex w-20 shrink-0 flex-col text-[11px] text-muted" aria-hidden="true">
				<div class="flex items-center px-2" :style="{ height: `${LANES.ruler}px` }">{{ t("cut.timeline.lanes.time") }}</div>
				<div class="flex items-center px-2" :style="{ height: `${LANES.thumbs}px` }">{{ t("cut.timeline.lanes.thumbnails") }}</div>
				<div class="flex items-center px-2" :style="{ height: `${LANES.wave}px` }">{{ t("cut.timeline.lanes.waveform") }}</div>
				<div class="flex items-center px-2" :style="{ height: `${LANES.segments}px` }">{{ t("cut.timeline.lanes.segments") }}</div>
				<div class="flex items-center px-2" :style="{ height: `${LANES.keyframes}px` }">{{ t("cut.timeline.lanes.keyframes") }}</div>
			</div>

			<div
				ref="lanes"
				class="relative min-w-0 flex-1 touch-none overflow-hidden"
				:class="drag?.kind === 'move' ? 'cursor-grabbing' : drag?.kind === 'resize' ? 'cursor-ew-resize' : 'cursor-crosshair'"
				@wheel.prevent="onWheel"
				@pointerdown="startSeek"
				@pointermove="onPointerMove"
				@pointerup="onPointerUp"
				@pointercancel="onPointerUp"
			>
				<div class="relative border-b border-border font-mono text-[10px] text-muted tabular-nums" :style="{ height: `${LANES.ruler}px` }">
					<template v-for="tick in rulerTicks" :key="tick.seconds">
						<div class="absolute bottom-0 w-px bg-border" :class="tick.major ? 'h-3' : 'h-1.5'" :style="{ left: `${tick.x}px` }" />
						<span v-if="tick.major" class="absolute top-0.5 pl-1 whitespace-nowrap" :style="{ left: `${tick.x}px` }">
							{{ tickLabel(tick.seconds, step) }}
						</span>
					</template>
				</div>

				<div class="thumbs relative overflow-hidden" :style="{ height: `${LANES.thumbs}px` }">
					<div
						v-for="slot in slots"
						:key="slot.seconds"
						class="absolute top-0 h-full bg-no-repeat"
						:style="{
							left: `${slot.x}px`,
							width: `${sheet!.tileWidth * thumbScale}px`,
							backgroundImage: `url(${spriteUrl(sheet!, slot.tile.sprite)})`,
							backgroundPosition: `-${slot.tile.offsetX * thumbScale}px -${slot.tile.offsetY * thumbScale}px`,
							backgroundSize: `${sheet!.columns * sheet!.tileWidth * thumbScale}px ${sheet!.rows * sheet!.tileHeight * thumbScale}px`
						}"
					/>
					<p v-if="!sheet || spritesReady === 0" class="absolute inset-0 flex items-center justify-center text-xs text-muted">
						{{ sheet ? t("cut.timeline.thumbnailsLoading") : "" }}
					</p>
				</div>

				<canvas ref="waveCanvas" class="block w-full bg-surface-sunken/40" :style="{ height: `${LANES.wave}px` }" />

				<div
					ref="segmentLane"
					class="relative border-y border-border"
					:style="{ height: `${LANES.segments}px` }"
					role="listbox"
					:aria-label="t('cut.timeline.lanes.segments')"
					@pointerdown="startCreate"
				>
					<div
						v-for="segment in shown"
						:key="segment.id"
						:role="segment.id === 0 ? undefined : 'option'"
						:tabindex="segment.id === 0 ? undefined : segment.id === focusId ? 0 : -1"
						:aria-selected="segment.id === 0 ? undefined : segment.id === selectedId"
						:aria-label="segment.id === 0 ? undefined : segmentName(segment)"
						@pointerdown="startSegment($event, segment)"
						@focus="emit('update:selectedId', segment.id)"
						@keydown="onSegmentKey($event, segment)"
						class="segment absolute inset-y-1 flex items-center overflow-hidden rounded-sm border text-[11px] whitespace-nowrap focus-visible:outline-2 focus-visible:outline-accent"
						:class="[
							removeMode ? 'segment-remove' : 'segment-keep',
							segment.id === selectedId || segment.id === 0 ? 'border-accent ring-1 ring-accent' : 'border-accent/60',
							drag?.kind === 'move' ? 'cursor-grabbing' : 'cursor-grab'
						]"
						:style="{ left: `${segment.x}px`, width: `${Math.max(segment.w, 2)}px` }"
						:title="`${formatTimecode(segment.start)} – ${formatTimecode(segment.end)}`"
					>
						<span class="absolute inset-y-0 left-0 w-2 cursor-ew-resize" @pointerdown="startSegment($event, segment, 'start')" />
						<span class="pointer-events-none px-2">
							{{ segment.id === 0 ? "" : t("cut.timeline.segmentLabel", { n: segment.index }) }}
							<span v-if="segment.w > 150" class="text-muted">· {{ formatTimecode(segment.end - segment.start) }}</span>
						</span>
						<span class="absolute inset-y-0 right-0 w-2 cursor-ew-resize" @pointerdown="startSegment($event, segment, 'end')" />
					</div>
					<p
						v-if="segments.length === 0 && !preview"
						class="pointer-events-none absolute inset-0 flex items-center justify-center text-xs text-muted"
					>
						{{ t("cut.timeline.dragHint") }}
					</p>
				</div>

				<canvas ref="keyframeCanvas" class="block w-full" :style="{ height: `${LANES.keyframes}px` }" />

				<div
					v-if="playheadX >= 0 && playheadX <= view.width"
					class="pointer-events-none absolute inset-y-0 w-px bg-danger"
					:style="{ left: `${playheadX}px` }"
					aria-hidden="true"
				/>
			</div>
		</div>

		<div class="ml-20 mr-0 h-2 rounded bg-surface-sunken" :title="t('cut.timeline.scrollHint')" aria-hidden="true">
			<div
				class="h-full cursor-grab rounded bg-muted/50 hover:bg-muted"
				:style="{ marginLeft: `${scrollbar.left}%`, width: `${scrollbar.width}%` }"
				@pointerdown="startScrollbar"
			/>
		</div>
	</section>
</template>

<style scoped>
/* Thumbnails sit on black like the player's stage */
.thumbs {
	background: #000;
}
.segment-keep {
	background: color-mix(in srgb, var(--mmt-accent) 25%, transparent);
}
.segment-remove {
	background: repeating-linear-gradient(
		135deg,
		color-mix(in srgb, var(--mmt-danger) 30%, transparent) 0 4px,
		color-mix(in srgb, var(--mmt-danger) 8%, transparent) 4px 8px
	);
	border-color: color-mix(in srgb, var(--mmt-danger) 60%, transparent);
}
</style>
