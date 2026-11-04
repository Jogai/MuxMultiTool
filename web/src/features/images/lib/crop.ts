import { create } from "@bufbuild/protobuf";

import { type CropOperation, CropOperationSchema, type CropSource, type CropWindow } from "~/generated/muxmultitool/crop/v1/crop_pb";

/** How much to hide on each edge, in pixels of the picture as it is shown now. */
export interface CropValues {
	top: number;
	bottom: number;
	left: number;
	right: number;
}

export type CropEdge = keyof CropValues;

export const EMPTY_CROP: CropValues = { top: 0, bottom: 0, left: 0, right: 0 };

/** The least a crop must leave of the picture; the server refuses less (INVALID_ARGUMENT). */
export const MIN_PICTURE_PX = 16;

export function isEmptyCrop(values: CropValues): boolean {
	return values.top === 0 && values.bottom === 0 && values.left === 0 && values.right === 0;
}

/** The nearest multiple of `unit`, never negative. */
export function snapValue(value: number, unit: number): number {
	const step = unit > 0 ? unit : 1;
	return Math.max(0, Math.round(value / step) * step);
}

/**
 * The values snapped to the stream's crop unit (2 px for 4:2:0 video) and kept inside the picture:
 * each pair of opposite sides leaves at least `MIN_PICTURE_PX`, the later side giving way. The
 * server snaps again on the plan and the submit; this keeps the inputs and the overlay honest first.
 */
export function snapCrop(values: CropValues, source: Pick<CropSource, "width" | "height" | "unitX" | "unitY">): CropValues {
	const top = snapValue(values.top, source.unitY);
	const left = snapValue(values.left, source.unitX);
	const maxBottom = Math.max(0, source.height - MIN_PICTURE_PX - top);
	const maxRight = Math.max(0, source.width - MIN_PICTURE_PX - left);
	return {
		top: Math.min(top, Math.max(0, source.height - MIN_PICTURE_PX)),
		bottom: Math.min(snapValue(values.bottom, source.unitY), snapValue(maxBottom, source.unitY)),
		left: Math.min(left, Math.max(0, source.width - MIN_PICTURE_PX)),
		right: Math.min(snapValue(values.right, source.unitX), snapValue(maxRight, source.unitX))
	};
}

/** The most an edge can take, as `snapCrop` allows it: the picture keeps `MIN_PICTURE_PX`, the later side giving way. */
export function edgeMax(values: CropValues, edge: CropEdge, source: Pick<CropSource, "width" | "height" | "unitX" | "unitY">): number {
	switch (edge) {
		case "top":
			return Math.max(0, source.height - MIN_PICTURE_PX);
		case "bottom":
			return snapValue(Math.max(0, source.height - MIN_PICTURE_PX - values.top), source.unitY);
		case "left":
			return Math.max(0, source.width - MIN_PICTURE_PX);
		default:
			return snapValue(Math.max(0, source.width - MIN_PICTURE_PX - values.left), source.unitX);
	}
}

/** The values with one edge changed, then snapped. */
export function withEdge(
	values: CropValues,
	edge: CropEdge,
	value: number,
	source: Pick<CropSource, "width" | "height" | "unitX" | "unitY">
): CropValues {
	return snapCrop({ ...values, [edge]: value }, source);
}

/** What a key press on a handle needs from its event. */
type NudgeKeys = Pick<KeyboardEvent, "key" | "preventDefault" | "stopPropagation">;

/**
 * The crop after an arrow key on an edge's handle (inward grows the crop), or `undefined` for any other
 * key. A handled key is the handle's alone: it is kept from the page (scrolling) and from the player
 * behind the picture, whose arrows step frames.
 */
export function nudgeCrop(
	values: CropValues,
	edge: CropEdge,
	event: NudgeKeys,
	source: Pick<CropSource, "width" | "height" | "unitX" | "unitY">
): CropValues | undefined {
	const unit = edge === "top" || edge === "bottom" ? source.unitY || 1 : source.unitX || 1;
	const grow = edge === "top" || edge === "left" ? ["ArrowDown", "ArrowRight"] : ["ArrowUp", "ArrowLeft"];
	const shrink = edge === "top" || edge === "left" ? ["ArrowUp", "ArrowLeft"] : ["ArrowDown", "ArrowRight"];
	const delta = grow.includes(event.key) ? unit : shrink.includes(event.key) ? -unit : 0;
	if (delta === 0) {
		return undefined;
	}
	event.preventDefault();
	event.stopPropagation();
	return withEdge(values, edge, values[edge] + delta, source);
}

/** The size the picture will show after the crop. */
export function croppedSize(values: CropValues, source: Pick<CropSource, "width" | "height">): { width: number; height: number } {
	return { width: Math.max(0, source.width - values.left - values.right), height: Math.max(0, source.height - values.top - values.bottom) };
}

/** `2.04:1` for 3840×1880. */
export function aspectLabel(width: number, height: number): string {
	return height > 0 ? `${(width / height).toFixed(2)}:1` : "–";
}

/** The four bars as percentages of the picture, for the overlay: how much of each edge is hidden. */
export function overlayBars(values: CropValues, source: Pick<CropSource, "width" | "height">): Record<CropEdge, number> {
	const pct = (value: number, total: number) => (total > 0 ? Math.min(100, (100 * value) / total) : 0);
	return {
		top: pct(values.top, source.height),
		bottom: pct(values.bottom, source.height),
		left: pct(values.left, source.width),
		right: pct(values.right, source.width)
	};
}

/**
 * The box the picture fills inside a stage that keeps the stage's aspect ratio (the player's stage is
 * 16:9 and the video is letterboxed into it): the offset and size in stage pixels.
 */
export function pictureBox(
	stage: { width: number; height: number },
	picture: { width: number; height: number }
): { x: number; y: number; width: number; height: number } {
	if (stage.width <= 0 || stage.height <= 0 || picture.width <= 0 || picture.height <= 0) {
		return { x: 0, y: 0, width: stage.width, height: stage.height };
	}
	const aspect = picture.width / picture.height;
	if (aspect >= stage.width / stage.height) {
		const height = stage.width / aspect;
		return { x: 0, y: (stage.height - height) / 2, width: stage.width, height };
	}
	const width = stage.height * aspect;
	return { x: (stage.width - width) / 2, y: 0, width, height: stage.height };
}

/**
 * The value of an edge after dragging it by `delta` stage pixels (x for left and right, y for top and
 * bottom), given the scale from stage pixels to picture pixels. Dragging the top edge down or the
 * bottom edge up grows the crop.
 */
export function draggedEdge(edge: CropEdge, startValue: number, delta: number, scale: number): number {
	const grows = edge === "top" || edge === "left" ? delta : -delta;
	return Math.max(0, startValue + grows * scale);
}

export function toCropValues(window: CropWindow | undefined): CropValues {
	return window ? { top: window.top, bottom: window.bottom, left: window.left, right: window.right } : EMPTY_CROP;
}

/** The operation the plan and the submit both take. */
export function toCropOperation(input: string, output: string, values: CropValues): CropOperation {
	return create(CropOperationSchema, { input, output: output.trim(), crop: values });
}

export function sameCrop(a: CropValues, b: CropValues): boolean {
	return a.top === b.top && a.bottom === b.bottom && a.left === b.left && a.right === b.right;
}

/**
 * Whether the crop may be submitted: the file is supported and something is cropped. A plan still in
 * flight does not hold the button back: the server snaps and checks the values again on the submit,
 * and a button that goes disabled while the input it follows loses focus would swallow the click.
 */
export function canCrop(supported: boolean | undefined, values: CropValues): boolean {
	return supported === true && !isEmptyCrop(values);
}
