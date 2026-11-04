import { describe, expect, it } from "vitest";

import {
	formatSegmentShortcut,
	formatTimelineShortcut,
	nudgeSegment,
	SEGMENT_SHORTCUT_ROWS,
	SEGMENT_SHORTCUTS,
	segmentShortcutAction,
	TIMELINE_ACTIONS,
	timelineShortcutAction
} from "./shortcuts";

const press = (key: string, modifiers: Partial<{ shift: boolean; ctrl: boolean; meta: boolean; alt: boolean }> = {}) => ({
	key,
	shiftKey: modifiers.shift ?? false,
	ctrlKey: modifiers.ctrl ?? false,
	metaKey: modifiers.meta ?? false,
	altKey: modifiers.alt ?? false
});

describe("timeline shortcuts", () => {
	it("maps undo and redo with Ctrl or Cmd", () => {
		expect(timelineShortcutAction(press("z", { ctrl: true }))).toBe("undo");
		expect(timelineShortcutAction(press("Z", { meta: true, shift: true }))).toBe("redo");
		expect(timelineShortcutAction(press("y", { ctrl: true }))).toBe("redo");
		expect(timelineShortcutAction(press("z"))).toBeUndefined();
	});

	it("zooms with + and - whatever the Shift state", () => {
		expect(timelineShortcutAction(press("+", { shift: true }))).toBe("zoomIn");
		expect(timelineShortcutAction(press("="))).toBe("zoomIn");
		expect(timelineShortcutAction(press("-"))).toBe("zoomOut");
		expect(timelineShortcutAction(press("0"))).toBe("zoomFit");
	});

	it("edits segments", () => {
		expect(timelineShortcutAction(press("Delete"))).toBe("deleteSegment");
		expect(timelineShortcutAction(press("Backspace"))).toBe("deleteSegment");
		expect(timelineShortcutAction(press("a"))).toBe("addSegment");
		expect(timelineShortcutAction(press("s"))).toBe("splitAtPlayhead");
		expect(timelineShortcutAction(press("a", { ctrl: true }))).toBeUndefined();
	});

	it("leaves Alt combinations and other keys alone", () => {
		expect(timelineShortcutAction(press("z", { ctrl: true, alt: true }))).toBeUndefined();
		expect(timelineShortcutAction(press("ArrowLeft"))).toBeUndefined();
	});

	it("formats the first shortcut of each action", () => {
		expect(formatTimelineShortcut("undo")).toBe("Ctrl + Z");
		expect(formatTimelineShortcut("redo", { mac: true })).toBe("⌘ + Shift + Z");
		expect(formatTimelineShortcut("zoomIn")).toBe("+");
		expect(TIMELINE_ACTIONS).toEqual(["undo", "redo", "zoomIn", "zoomOut", "zoomFit", "addSegment", "splitAtPlayhead", "deleteSegment"]);
	});
});

describe("segment shortcuts", () => {
	it("moves the segment with the arrows, its start with Ctrl and its end with Shift", () => {
		expect(segmentShortcutAction(press("ArrowLeft"))).toBe("moveBack");
		expect(segmentShortcutAction(press("ArrowRight"))).toBe("moveForward");
		expect(segmentShortcutAction(press("ArrowLeft", { ctrl: true }))).toBe("startBack");
		expect(segmentShortcutAction(press("ArrowRight", { meta: true }))).toBe("startForward");
		expect(segmentShortcutAction(press("ArrowLeft", { shift: true }))).toBe("endBack");
		expect(segmentShortcutAction(press("ArrowRight", { shift: true }))).toBe("endForward");
	});

	it("moves the focus with Up and Down and leaves other keys to the host", () => {
		expect(segmentShortcutAction(press("ArrowUp"))).toBe("previous");
		expect(segmentShortcutAction(press("ArrowDown"))).toBe("next");
		expect(segmentShortcutAction(press("ArrowLeft", { alt: true }))).toBeUndefined();
		expect(segmentShortcutAction(press("Delete"))).toBeUndefined();
		expect(segmentShortcutAction(press("Home"))).toBeUndefined();
	});

	it("nudges the body by a step and keeps it inside the file", () => {
		const segment = { start: 10, end: 12 };
		expect(nudgeSegment(segment, "moveForward", 0.5, 60, 0.04)).toEqual({ kind: "move", start: 10.5 });
		expect(nudgeSegment(segment, "moveBack", 0.5, 60, 0.04)).toEqual({ kind: "move", start: 9.5 });
		expect(nudgeSegment({ start: 0.2, end: 2 }, "moveBack", 0.5, 60, 0.04)).toEqual({ kind: "move", start: 0 });
		expect(nudgeSegment({ start: 58, end: 59 }, "moveForward", 2, 60, 0.04)).toEqual({ kind: "move", start: 59 });
	});

	it("nudges an edge and keeps the segment at least the minimum length", () => {
		const segment = { start: 10, end: 12 };
		expect(nudgeSegment(segment, "startForward", 0.5, 60, 0.04)).toEqual({ kind: "resize", edge: "start", seconds: 10.5 });
		expect(nudgeSegment(segment, "endBack", 0.5, 60, 0.04)).toEqual({ kind: "resize", edge: "end", seconds: 11.5 });
		expect(nudgeSegment(segment, "startForward", 5, 60, 0.5)).toEqual({ kind: "resize", edge: "start", seconds: 11.5 });
		expect(nudgeSegment(segment, "endBack", 5, 60, 0.5)).toEqual({ kind: "resize", edge: "end", seconds: 10.5 });
		expect(nudgeSegment(segment, "startBack", 20, 60, 0.04)).toEqual({ kind: "resize", edge: "start", seconds: 0 });
		expect(nudgeSegment(segment, "endForward", 100, 60, 0.04)).toEqual({ kind: "resize", edge: "end", seconds: 60 });
	});

	it("lists every segment key once, in rows of back and forward", () => {
		const listed = SEGMENT_SHORTCUT_ROWS.flatMap((row) => row.actions);
		expect(listed).toEqual(SEGMENT_SHORTCUTS.map((shortcut) => shortcut.action));
		expect(formatSegmentShortcut("startBack")).toBe("Ctrl + ←");
		expect(formatSegmentShortcut("startBack", { mac: true })).toBe("⌘ + ←");
		expect(formatSegmentShortcut("endForward")).toBe("Shift + →");
		expect(formatSegmentShortcut("next")).toBe("↓");
	});
});
