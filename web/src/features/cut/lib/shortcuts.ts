import { formatShortcut, type KeyInput, matchShortcut, type Shortcut } from "~/core/libs/shortcuts";

import type { Segment } from "./segments";

/**
 * The fixed keyboard shortcuts of the timeline and segment editor (the gui/04-cut sketch notes: Ctrl+Z /
 * Ctrl+Shift+Z undo and redo, +/- zoom). The player's own shortcuts are in `features/player/lib/shortcuts`;
 * the two sets share no key. A focused segment takes the arrow keys for itself (`SEGMENT_SHORTCUTS`),
 * so the player's frame stepping does not run while one is being nudged.
 */
export type TimelineAction = "undo" | "redo" | "zoomIn" | "zoomOut" | "zoomFit" | "addSegment" | "deleteSegment" | "splitAtPlayhead";

/** What a key does to the focused segment: moves it, one of its edges, or the focus to a neighbour. */
export type SegmentAction = "moveBack" | "moveForward" | "startBack" | "startForward" | "endBack" | "endForward" | "previous" | "next";

/** The keys of a focused segment, in the order the shortcut list shows them. */
export const SEGMENT_SHORTCUTS: readonly Shortcut<SegmentAction>[] = [
	{ action: "moveBack", key: "ArrowLeft" },
	{ action: "moveForward", key: "ArrowRight" },
	{ action: "startBack", key: "ArrowLeft", ctrl: true },
	{ action: "startForward", key: "ArrowRight", ctrl: true },
	{ action: "endBack", key: "ArrowLeft", shift: true },
	{ action: "endForward", key: "ArrowRight", shift: true },
	{ action: "previous", key: "ArrowUp" },
	{ action: "next", key: "ArrowDown" }
];

/** The rows of the shortcut list for a focused segment: each pairs the key for back and forward. */
export const SEGMENT_SHORTCUT_ROWS: ReadonlyArray<{ key: "move" | "start" | "end" | "select"; actions: readonly SegmentAction[] }> = [
	{ key: "move", actions: ["moveBack", "moveForward"] },
	{ key: "start", actions: ["startBack", "startForward"] },
	{ key: "end", actions: ["endBack", "endForward"] },
	{ key: "select", actions: ["previous", "next"] }
];

/** The mapping, in the order the shortcut list shows it. A key listed twice is an alternative. */
export const TIMELINE_SHORTCUTS: readonly Shortcut<TimelineAction>[] = [
	{ action: "undo", key: "z", ctrl: true },
	{ action: "redo", key: "z", ctrl: true, shift: true },
	{ action: "redo", key: "y", ctrl: true },
	{ action: "zoomIn", key: "+" },
	{ action: "zoomIn", key: "=" },
	{ action: "zoomOut", key: "-" },
	{ action: "zoomFit", key: "0" },
	{ action: "addSegment", key: "a" },
	{ action: "splitAtPlayhead", key: "s" },
	{ action: "deleteSegment", key: "Delete" },
	{ action: "deleteSegment", key: "Backspace" }
];

/** The action a key press maps to, or `undefined` when it is not a timeline shortcut. */
export function timelineShortcutAction(event: KeyInput): TimelineAction | undefined {
	return matchShortcut(TIMELINE_SHORTCUTS, event)?.action;
}

/** The action a key press on a focused segment maps to, or `undefined` when it is not one of its keys. */
export function segmentShortcutAction(event: KeyInput): SegmentAction | undefined {
	return matchShortcut(SEGMENT_SHORTCUTS, event)?.action;
}

/** What a nudge asks of the host: the same as a drag of the segment's body or of one edge. */
export type SegmentNudge = { kind: "move"; start: number } | { kind: "resize"; edge: "start" | "end"; seconds: number };

/**
 * The segment, or one of its edges, moved by `step` seconds: kept inside the file and at least
 * `minLength` long, as a drag is. `previous` and `next` move the focus, not the segment, so they are
 * not nudges.
 */
export function nudgeSegment(
	segment: Pick<Segment, "start" | "end">,
	action: Exclude<SegmentAction, "previous" | "next">,
	step: number,
	duration: number,
	minLength: number
): SegmentNudge {
	const delta = action.endsWith("Forward") ? step : -step;
	const clamp = (value: number, low: number, high: number) => Math.max(low, Math.min(value, high));
	switch (action) {
		case "moveBack":
		case "moveForward":
			return { kind: "move", start: clamp(segment.start + delta, 0, Math.max(0, duration - (segment.end - segment.start))) };
		case "startBack":
		case "startForward":
			return { kind: "resize", edge: "start", seconds: clamp(segment.start + delta, 0, segment.end - minLength) };
		default:
			return { kind: "resize", edge: "end", seconds: clamp(segment.end + delta, segment.start + minLength, duration) };
	}
}

/** The first shortcut of an action, as the list shows it. */
export function formatTimelineShortcut(action: TimelineAction, platform?: { mac: boolean }): string {
	const shortcut = TIMELINE_SHORTCUTS.find((candidate) => candidate.action === action);
	return shortcut ? formatShortcut(shortcut, platform) : "";
}

/** The key of a focused segment's action, as the list shows it. */
export function formatSegmentShortcut(action: SegmentAction, platform?: { mac: boolean }): string {
	const shortcut = SEGMENT_SHORTCUTS.find((candidate) => candidate.action === action);
	return shortcut ? formatShortcut(shortcut, platform) : "";
}

/** The actions in display order, each once. */
export const TIMELINE_ACTIONS: readonly TimelineAction[] = [...new Set(TIMELINE_SHORTCUTS.map((shortcut) => shortcut.action))];
