<script setup lang="ts">
import { type CropEdge, type CropValues, draggedEdge, edgeMax, nudgeCrop, overlayBars, pictureBox, withEdge } from "../lib/crop";
import type { CropSource } from "~/generated/muxmultitool/crop/v1/crop_pb";

/**
 * The crop drawn over the player's picture (sketch 07): a shaded bar per edge with its size, and a
 * handle on each bar's inner edge to drag it, snapped to the crop unit. Each handle is a slider for
 * the keyboard: the arrows nudge it by one unit, Home and End take it to nothing and to the most it
 * can take. The player's stage is 16:9 and letterboxes the video, so the overlay first finds the
 * picture's box inside the stage.
 */
const { crop, source } = defineProps<{ crop: CropValues; source: Pick<CropSource, "width" | "height" | "unitX" | "unitY"> }>();
const emit = defineEmits<{ "update:crop": [values: CropValues] }>();
const { t } = useI18n();

const stage = useTemplateRef<HTMLElement>("stage");
const { width: stageWidth, height: stageHeight } = useElementSize(stage);
const box = computed(() => pictureBox({ width: stageWidth.value, height: stageHeight.value }, source));
const bars = computed(() => overlayBars(crop, source));
/** Picture pixels per stage pixel. */
const scale = computed(() => (box.value.width > 0 ? source.width / box.value.width : 1));

const edges: readonly CropEdge[] = ["top", "bottom", "left", "right"];
function barStyle(edge: CropEdge): Record<string, string> {
	const size = `${bars.value[edge]}%`;
	switch (edge) {
		case "top":
			return { top: "0", left: "0", right: "0", height: size };
		case "bottom":
			return { bottom: "0", left: "0", right: "0", height: size };
		case "left":
			return { top: "0", bottom: "0", left: "0", width: size };
		default:
			return { top: "0", bottom: "0", right: "0", width: size };
	}
}
function handleStyle(edge: CropEdge): Record<string, string> {
	const at = `${bars.value[edge]}%`;
	switch (edge) {
		case "top":
			return { top: at, left: "0", right: "0", height: "8px", marginTop: "-4px", cursor: "ns-resize" };
		case "bottom":
			return { bottom: at, left: "0", right: "0", height: "8px", marginBottom: "-4px", cursor: "ns-resize" };
		case "left":
			return { left: at, top: "0", bottom: "0", width: "8px", marginLeft: "-4px", cursor: "ew-resize" };
		default:
			return { right: at, top: "0", bottom: "0", width: "8px", marginRight: "-4px", cursor: "ew-resize" };
	}
}

// Dragging a handle: the edge's value follows the pointer, snapped on every move
const dragging = ref<CropEdge>();
let origin = 0;
let startValue = 0;
function startDrag(edge: CropEdge, event: PointerEvent) {
	dragging.value = edge;
	origin = edge === "top" || edge === "bottom" ? event.clientY : event.clientX;
	startValue = crop[edge];
	(event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
}
function moveDrag(event: PointerEvent) {
	const edge = dragging.value;
	if (!edge) {
		return;
	}
	const delta = (edge === "top" || edge === "bottom" ? event.clientY : event.clientX) - origin;
	emit("update:crop", withEdge(crop, edge, draggedEdge(edge, startValue, delta, scale.value), source));
}
function endDrag() {
	dragging.value = undefined;
}
/** Home and End: the edge takes nothing, or the most it can. The player seeks on the same keys, so they stop here. */
function jump(edge: CropEdge, event: KeyboardEvent, value: number) {
	event.stopPropagation();
	emit("update:crop", withEdge(crop, edge, value, source));
}
function nudge(edge: CropEdge, event: KeyboardEvent) {
	const next = nudgeCrop(crop, edge, event, source);
	if (next) {
		emit("update:crop", next);
	}
}
</script>

<template>
	<div ref="stage" class="pointer-events-none absolute inset-0">
		<div class="absolute" :style="{ left: `${box.x}px`, top: `${box.y}px`, width: `${box.width}px`, height: `${box.height}px` }">
			<div v-for="edge in edges" :key="edge" class="bar absolute flex items-center justify-center overflow-hidden" :style="barStyle(edge)">
				<span v-if="crop[edge] > 0" class="label rounded px-1.5 py-0.5 text-xs whitespace-nowrap">{{
					t("images.crop.bar", { n: crop[edge] })
				}}</span>
			</div>
			<div
				v-for="edge in edges"
				:key="`handle-${edge}`"
				role="slider"
				tabindex="0"
				class="handle pointer-events-auto absolute focus-visible:outline-2 focus-visible:outline-accent"
				:class="{ active: dragging === edge }"
				:style="handleStyle(edge)"
				:aria-label="t('images.crop.dragEdge', { edge: t(`images.crop.${edge}`) })"
				:aria-valuenow="crop[edge]"
				aria-valuemin="0"
				:aria-valuemax="edgeMax(crop, edge, source)"
				:aria-valuetext="t('images.crop.edgeValue', { n: crop[edge] })"
				:aria-orientation="edge === 'top' || edge === 'bottom' ? 'vertical' : 'horizontal'"
				@pointerdown="startDrag(edge, $event)"
				@pointermove="moveDrag"
				@pointerup="endDrag"
				@pointercancel="endDrag"
				@keydown="nudge(edge, $event)"
				@keydown.home.prevent="jump(edge, $event, 0)"
				@keydown.end.prevent="jump(edge, $event, edgeMax(crop, edge, source))"
			/>
		</div>
	</div>
</template>

<style scoped>
/* Over the picture, not themed: the bars are the hidden area, the handles its edges */
.bar {
	background: rgb(0 0 0 / 0.55);
	box-shadow: inset 0 0 0 1px rgb(255 255 255 / 0.35);
}
.label {
	color: #fff;
	background: rgb(0 0 0 / 0.6);
}
.handle {
	background: transparent;
}
.handle:hover,
.handle.active {
	background: rgb(255 255 255 / 0.35);
}
</style>
