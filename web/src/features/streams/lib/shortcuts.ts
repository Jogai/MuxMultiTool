import { DIGITS, type Shortcut } from "~/core/libs/shortcuts";

/**
 * The fixed keyboard shortcuts of the Streams and Combine screens (mvp-features.md, "Fixed keyboard
 * shortcuts"). Each screen has its own set, listed in a dialog opened with `?`. The player's shortcuts
 * live with the player.
 */

export type StreamsShortcutAction =
	| "queue"
	| "previousFile"
	| "nextFile"
	| "toggleStream"
	| "toggleVideoOnly"
	| "toggleExtract"
	| "resetTicks"
	| "newRule"
	| "copyCommand"
	| "showShortcuts";

export type CombineShortcutAction = "queue" | "addSource" | "trackUp" | "trackDown" | "removeTrack" | "copyCommand" | "showShortcuts";

/** The Streams screen's mapping, in the order the list shows it. */
export const STREAMS_SHORTCUTS: readonly Shortcut<StreamsShortcutAction>[] = [
	{ action: "toggleStream", key: DIGITS },
	{ action: "previousFile", key: "[" },
	{ action: "nextFile", key: "]" },
	{ action: "toggleVideoOnly", key: "v" },
	{ action: "toggleExtract", key: "x" },
	{ action: "resetTicks", key: "r" },
	{ action: "newRule", key: "n" },
	{ action: "copyCommand", key: "c" },
	{ action: "queue", key: "Enter", ctrl: true },
	{ action: "showShortcuts", key: "?" }
];

/** The Combine screen's mapping, in the order the list shows it. */
export const COMBINE_SHORTCUTS: readonly Shortcut<CombineShortcutAction>[] = [
	{ action: "addSource", key: "a" },
	{ action: "trackUp", key: "ArrowUp", shift: true },
	{ action: "trackDown", key: "ArrowDown", shift: true },
	{ action: "removeTrack", key: "Delete" },
	{ action: "copyCommand", key: "c" },
	{ action: "queue", key: "Enter", ctrl: true },
	{ action: "showShortcuts", key: "?" }
];
