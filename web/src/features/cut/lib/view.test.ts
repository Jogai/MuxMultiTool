import { describe, expect, it } from "vitest";

import {
	clampScroll,
	contentWidth,
	fitView,
	fitZoom,
	maxZoom,
	PX_PER_FRAME_AT_MAX_ZOOM,
	resizeView,
	scrollByPx,
	scrollIntoView,
	tickLabel,
	tickStep,
	ticks,
	timeToX,
	visibleRange,
	xToTime,
	zoomAtFraction,
	zoomBy,
	zoomFraction,
	zoomTo
} from "./view";

describe("time/pixel mapping", () => {
	const view = fitView(600, 1200); // 2 px per second

	it("fits the whole file at the start", () => {
		expect(view.pxPerSecond).toBe(2);
		expect(contentWidth(view)).toBe(1200);
		expect(visibleRange(view)).toEqual({ from: 0, to: 600 });
		expect(fitZoom(0, 1000)).toBe(1);
	});

	it("maps times to pixels and back", () => {
		expect(timeToX(view, 30)).toBe(60);
		expect(xToTime(view, 60)).toBe(30);
		const scrolled = { ...view, pxPerSecond: 10, scroll: 100 };
		expect(timeToX(scrolled, 100)).toBe(0);
		expect(timeToX(scrolled, 50)).toBe(-500);
		expect(xToTime(scrolled, 1200)).toBe(220);
	});

	it("clamps pixel positions to the file", () => {
		expect(xToTime(view, -50)).toBe(0);
		expect(xToTime(view, 5000)).toBe(600);
	});

	it("zooms down to single frames and never below the fit", () => {
		expect(maxZoom(25)).toBe(25 * PX_PER_FRAME_AT_MAX_ZOOM);
		expect(zoomTo(view, 0.5, 0).pxPerSecond).toBe(2);
		expect(zoomTo(view, 1e9, 0, 25).pxPerSecond).toBe(300);
		expect(zoomTo(view, 1e9, 0, 50).pxPerSecond).toBe(600);
	});

	it("keeps the time under the anchor in place when zooming", () => {
		const zoomed = zoomBy(view, 4, 600); // the anchor is at 300 s
		expect(zoomed.pxPerSecond).toBe(8);
		expect(xToTime(zoomed, 600)).toBeCloseTo(300);
		const back = zoomBy(zoomed, 0.25, 600);
		expect(back).toEqual(view);
	});

	it("keeps the scroll inside the file", () => {
		const zoomed = { ...view, pxPerSecond: 10 }; // 120 s visible
		expect(clampScroll({ ...zoomed, scroll: -5 }).scroll).toBe(0);
		expect(clampScroll({ ...zoomed, scroll: 1000 }).scroll).toBe(480);
		expect(scrollByPx({ ...zoomed, scroll: 100 }, 100).scroll).toBe(110);
		expect(clampScroll(view)).toBe(view);
	});

	it("scrolls only as far as needed to show a time", () => {
		const zoomed = { ...view, pxPerSecond: 10, scroll: 100 }; // shows 100..220
		expect(scrollIntoView(zoomed, 150)).toBe(zoomed);
		expect(scrollIntoView(zoomed, 50).scroll).toBe(50);
		expect(scrollIntoView(zoomed, 230).scroll).toBeCloseTo(218);
		expect(scrollIntoView(zoomed, 599).scroll).toBe(480);
	});

	it("keeps a fitted view fitted and a zoomed one zoomed on resize", () => {
		expect(resizeView(view, 600)).toEqual(fitView(600, 600));
		const zoomed = { ...view, pxPerSecond: 10, scroll: 100 };
		expect(resizeView(zoomed, 2400)).toEqual({ ...zoomed, width: 2400 });
		// Narrowing below the fit lifts the zoom to the new fit
		const wide = { ...view, pxPerSecond: 2.5, scroll: 0 };
		expect(resizeView(wide, 3000).pxPerSecond).toBe(5);
	});

	it("maps the zoom to a slider fraction and back", () => {
		expect(zoomFraction(view, 25)).toBe(0);
		expect(zoomFraction({ ...view, pxPerSecond: 300 }, 25)).toBe(1);
		const half = zoomAtFraction(view, 0.5, 25);
		expect(zoomFraction({ ...view, pxPerSecond: half }, 25)).toBeCloseTo(0.5);
	});
});

describe("ruler ticks", () => {
	it("picks the smallest step with labels far enough apart", () => {
		expect(tickStep(2)).toBe(60); // 60 s = 120 px
		expect(tickStep(10)).toBe(10);
		expect(tickStep(300)).toBe(0.5);
		expect(tickStep(0.01)).toBe(3600);
	});

	it("steps by frames when a frame is wider than the label spacing", () => {
		expect(tickStep(3000, 90, 25)).toBe(1 / 25);
		expect(tickStep(300, 90, 25)).toBe(0.5);
	});

	it("lists the visible ticks with the labelled ones marked", () => {
		const view = { duration: 600, width: 1200, pxPerSecond: 2, scroll: 0 };
		const result = ticks(view);
		const major = result.filter((tick) => tick.major).map((tick) => tick.seconds);
		expect(major).toEqual([0, 60, 120, 180, 240, 300, 360, 420, 480, 540, 600]);
		expect(
			result
				.filter((tick) => !tick.major)
				.map((tick) => tick.seconds)
				.slice(0, 2)
		).toEqual([30, 90]);
		expect(result[1]?.x).toBe(60);
	});

	it("never lists ticks outside the file", () => {
		const view = { duration: 100, width: 1200, pxPerSecond: 2, scroll: 0 };
		expect(ticks(view).every((tick) => tick.seconds >= 0 && tick.seconds <= 100)).toBe(true);
		expect(ticks({ ...view, duration: 0 })).toEqual([]);
	});
});

describe("tick labels", () => {
	it("shows a clock, with hours only when there are any and milliseconds for fine steps", () => {
		expect(tickLabel(0, 60)).toBe("0:00");
		expect(tickLabel(754, 10)).toBe("12:34");
		expect(tickLabel(3661, 60)).toBe("1:01:01");
		expect(tickLabel(1.5, 0.5)).toBe("0:01.500");
		expect(tickLabel(0.04, 0.04)).toBe("0:00.040");
	});
});
