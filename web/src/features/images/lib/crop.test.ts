import { create } from "@bufbuild/protobuf";
import { describe, expect, it, vi } from "vitest";

import {
	aspectLabel,
	canCrop,
	croppedSize,
	draggedEdge,
	edgeMax,
	EMPTY_CROP,
	isEmptyCrop,
	nudgeCrop,
	overlayBars,
	pictureBox,
	sameCrop,
	snapCrop,
	snapValue,
	toCropOperation,
	toCropValues,
	withEdge
} from "./crop";
import { CropWindowSchema } from "~/generated/muxmultitool/crop/v1/crop_pb";

const uhd = { width: 3840, height: 2160, unitX: 2, unitY: 2 };

describe("nudgeCrop", () => {
	const press = (key: string) => ({ key, preventDefault: vi.fn(), stopPropagation: vi.fn() });

	it("moves an edge one unit inward or outward and keeps the key to itself", () => {
		const crop = { top: 10, bottom: 0, left: 4, right: 0 };
		const down = press("ArrowDown");
		expect(nudgeCrop(crop, "top", down, uhd)?.top).toBe(12);
		// The player behind the picture steps frames on the arrows: the handle's keys must not reach it
		expect(down.preventDefault).toHaveBeenCalledOnce();
		expect(down.stopPropagation).toHaveBeenCalledOnce();
		expect(nudgeCrop(crop, "top", press("ArrowUp"), uhd)?.top).toBe(8);
		expect(nudgeCrop(crop, "bottom", press("ArrowUp"), uhd)?.bottom).toBe(2);
		expect(nudgeCrop(crop, "left", press("ArrowLeft"), uhd)?.left).toBe(2);
		expect(nudgeCrop(crop, "right", press("ArrowLeft"), uhd)?.right).toBe(2);
		expect(nudgeCrop(crop, "right", press("ArrowRight"), uhd)?.right).toBe(0);
	});

	it("leaves other keys alone", () => {
		const crop = { top: 10, bottom: 0, left: 4, right: 0 };
		for (const key of ["Enter", " ", "Home"]) {
			const event = press(key);
			expect(nudgeCrop(crop, "top", event, uhd)).toBeUndefined();
			expect(event.preventDefault).not.toHaveBeenCalled();
			expect(event.stopPropagation).not.toHaveBeenCalled();
		}
	});
});

describe("crop values", () => {
	it("snaps to the crop unit and never below zero", () => {
		expect(snapValue(61, 2)).toBe(62);
		expect(snapValue(140, 2)).toBe(140);
		expect(snapValue(3, 4)).toBe(4);
		expect(snapValue(-5, 2)).toBe(0);
		expect(snapValue(7, 0)).toBe(7);
	});

	it("keeps a picture of at least 16 px, the later side giving way", () => {
		expect(snapCrop({ top: 141, bottom: 139, left: 0, right: 0 }, uhd)).toEqual({ top: 142, bottom: 140, left: 0, right: 0 });
		expect(snapCrop({ top: 2160, bottom: 2160, left: 3840, right: 3840 }, uhd)).toEqual({ top: 2144, bottom: 0, left: 3824, right: 0 });
		expect(snapCrop({ top: 1000, bottom: 1200, left: 0, right: 0 }, uhd)).toEqual({ top: 1000, bottom: 1144, left: 0, right: 0 });
		expect(withEdge(EMPTY_CROP, "left", 9, uhd)).toEqual({ top: 0, bottom: 0, left: 10, right: 0 });
	});

	it("knows the most each edge can take, for the handles' range and End key", () => {
		expect(edgeMax(EMPTY_CROP, "top", uhd)).toBe(2144);
		expect(edgeMax(EMPTY_CROP, "left", uhd)).toBe(3824);
		expect(edgeMax({ top: 1000, bottom: 0, left: 0, right: 0 }, "bottom", uhd)).toBe(1144);
		expect(edgeMax({ top: 0, bottom: 0, left: 3825, right: 0 }, "right", uhd)).toBe(0);
		expect(withEdge(EMPTY_CROP, "top", edgeMax(EMPTY_CROP, "top", uhd), uhd).top).toBe(2144);
	});

	it("works out the result", () => {
		const bars = { top: 140, bottom: 140, left: 0, right: 0 };
		expect(croppedSize(bars, uhd)).toEqual({ width: 3840, height: 1880 });
		expect(aspectLabel(3840, 1880)).toBe("2.04:1");
		expect(aspectLabel(1, 0)).toBe("–");
		const pct = overlayBars(bars, uhd);
		expect(pct.top).toBeCloseTo(6.4815, 3);
		expect(pct.bottom).toBeCloseTo(6.4815, 3);
		expect(pct.left).toBe(0);
		expect(pct.right).toBe(0);
		expect(overlayBars(bars, { width: 0, height: 0 })).toEqual({ top: 0, bottom: 0, left: 0, right: 0 });
		expect(isEmptyCrop(EMPTY_CROP)).toBe(true);
		expect(isEmptyCrop(bars)).toBe(false);
	});

	it("places the picture inside the 16:9 stage", () => {
		expect(pictureBox({ width: 1600, height: 900 }, { width: 3840, height: 2160 })).toEqual({ x: 0, y: 0, width: 1600, height: 900 });
		const wide = pictureBox({ width: 1600, height: 900 }, { width: 2400, height: 1000 });
		expect(wide.x).toBe(0);
		expect(wide.width).toBe(1600);
		expect(wide.height).toBeCloseTo(666.667, 2);
		expect(wide.y).toBeCloseTo(116.667, 2);
		expect(pictureBox({ width: 1600, height: 900 }, { width: 1000, height: 1000 })).toEqual({ x: 350, y: 0, width: 900, height: 900 });
		expect(pictureBox({ width: 0, height: 0 }, { width: 1, height: 1 })).toEqual({ x: 0, y: 0, width: 0, height: 0 });
	});

	it("grows the crop when an edge is dragged inwards", () => {
		expect(draggedEdge("top", 100, 10, 2)).toBe(120);
		expect(draggedEdge("bottom", 100, 10, 2)).toBe(80);
		expect(draggedEdge("left", 0, -10, 2)).toBe(0);
		expect(draggedEdge("right", 50, -10, 2)).toBe(70);
	});

	it("converts to and from the messages and gates the submit", () => {
		const values = toCropValues(create(CropWindowSchema, { top: 140, bottom: 140 }));
		expect(values).toEqual({ top: 140, bottom: 140, left: 0, right: 0 });
		expect(toCropValues(undefined)).toEqual(EMPTY_CROP);
		expect(toCropOperation("in", " out.mkv ", values)).toMatchObject({ input: "in", output: "out.mkv", crop: values });
		expect(canCrop(true, values)).toBe(true);
		expect(canCrop(true, EMPTY_CROP)).toBe(false);
		expect(canCrop(false, values)).toBe(false);
		expect(canCrop(undefined, values)).toBe(false);
		expect(sameCrop(values, { ...values })).toBe(true);
		expect(sameCrop(values, { ...values, top: 142 })).toBe(false);
	});
});
