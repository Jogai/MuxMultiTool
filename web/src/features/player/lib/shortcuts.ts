import { type KeyInput, matchShortcut, type Shortcut } from "~/core/libs/shortcuts";

/**
 * The fixed keyboard shortcuts of the player (mvp-features.md, "Fixed keyboard shortcuts"; the gui/04-cut
 * sketch notes). The timeline's own shortcuts (undo/redo, zoom) live with the timeline.
 */

export type PlayerAction =
	| "togglePlay"
	| "frameBack"
	| "frameForward"
	| "keyframeBack"
	| "keyframeForward"
	| "seekBack"
	| "seekForward"
	| "seekStart"
	| "seekEnd"
	| "setRangeStart"
	| "setRangeEnd"
	| "playRange"
	| "toggleLoop"
	| "toggleMute"
	| "showShortcuts";

/** How far Ctrl+←/→ seek. */
export const SEEK_STEP_SECONDS = 5;

/** The mapping, in the order the shortcut list shows it. */
export const PLAYER_SHORTCUTS: readonly Shortcut<PlayerAction>[] = [
	{ action: "togglePlay", key: " " },
	{ action: "frameBack", key: "ArrowLeft" },
	{ action: "frameForward", key: "ArrowRight" },
	{ action: "keyframeBack", key: "ArrowLeft", shift: true },
	{ action: "keyframeForward", key: "ArrowRight", shift: true },
	{ action: "seekBack", key: "ArrowLeft", ctrl: true },
	{ action: "seekForward", key: "ArrowRight", ctrl: true },
	{ action: "seekStart", key: "Home" },
	{ action: "seekEnd", key: "End" },
	{ action: "setRangeStart", key: "i" },
	{ action: "setRangeEnd", key: "o" },
	{ action: "playRange", key: "p" },
	{ action: "toggleLoop", key: "l" },
	{ action: "toggleMute", key: "m" },
	{ action: "showShortcuts", key: "?" }
];

/** The action a key press maps to, or `undefined` when it is not a player shortcut. */
export function shortcutAction(event: KeyInput): PlayerAction | undefined {
	return matchShortcut(PLAYER_SHORTCUTS, event)?.action;
}
