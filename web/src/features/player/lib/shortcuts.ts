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

export interface Shortcut {
	action: PlayerAction;
	/** `KeyboardEvent.key`; letters are matched case-insensitively. */
	key: string;
	shift?: boolean;
	/** Ctrl, or Cmd on macOS. */
	ctrl?: boolean;
}

/** How far Ctrl+←/→ seek. */
export const SEEK_STEP_SECONDS = 5;

/** The mapping, in the order the shortcut list shows it. */
export const PLAYER_SHORTCUTS: readonly Shortcut[] = [
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

/** The parts of a keyboard event the mapping looks at. */
export type KeyInput = Pick<KeyboardEvent, "key" | "shiftKey" | "ctrlKey" | "metaKey" | "altKey">;

function sameKey(shortcut: Shortcut, key: string): boolean {
	return shortcut.key.length === 1 ? shortcut.key.toLowerCase() === key.toLowerCase() : shortcut.key === key;
}

/**
 * The action a key press maps to, or `undefined` when it is not a player shortcut. Alt never takes part
 * (it is the browser's). A `?` is typed with Shift on most layouts, so for shortcuts on a symbol the
 * Shift state is not checked.
 */
export function shortcutAction(event: KeyInput): PlayerAction | undefined {
	if (event.altKey) {
		return undefined;
	}
	const ctrl = event.ctrlKey || event.metaKey;
	for (const shortcut of PLAYER_SHORTCUTS) {
		if (!sameKey(shortcut, event.key) || (shortcut.ctrl ?? false) !== ctrl) {
			continue;
		}
		const symbol = shortcut.key.length === 1 && !/[a-z ]/i.test(shortcut.key);
		if (symbol || (shortcut.shift ?? false) === event.shiftKey) {
			return shortcut.action;
		}
	}
	return undefined;
}

/** Whether the key press belongs to a text field, where the player must stay out of the way. */
export function isTypingTarget(target: EventTarget | null): boolean {
	// Structural, not `instanceof HTMLElement`, so it works for any element and without a DOM in tests
	const element = target as { tagName?: unknown; isContentEditable?: unknown } | null;
	if (!element || typeof element.tagName !== "string") {
		return false;
	}
	return element.isContentEditable === true || TYPING_TAGS.has(element.tagName);
}

const TYPING_TAGS = new Set(["INPUT", "TEXTAREA", "SELECT"]);

/**
 * Whether the key press comes from inside a dialog, which has the keyboard to itself: a dialog over
 * the player (the cut editor's split dialog, say) must not play or step frames behind its back.
 */
export function isDialogTarget(target: EventTarget | null): boolean {
	const element = target as { closest?: unknown } | null;
	if (!element || typeof element.closest !== "function") {
		return false;
	}
	return (element.closest as (selector: string) => unknown)(DIALOG_SELECTOR) != null;
}

const DIALOG_SELECTOR = '[role="dialog"], [role="alertdialog"]';

const KEY_LABELS: Record<string, string> = {
	" ": "Space",
	ArrowLeft: "←",
	ArrowRight: "→",
	ArrowUp: "↑",
	ArrowDown: "↓"
};

/** The shortcut as the list shows it, e.g. `Shift + →` or `Ctrl + ←`. */
export function formatShortcut(shortcut: Shortcut, platform: { mac: boolean } = { mac: false }): string {
	const parts: string[] = [];
	if (shortcut.ctrl) {
		parts.push(platform.mac ? "⌘" : "Ctrl");
	}
	if (shortcut.shift) {
		parts.push("Shift");
	}
	const key = KEY_LABELS[shortcut.key] ?? (shortcut.key.length === 1 ? shortcut.key.toUpperCase() : shortcut.key);
	parts.push(key);
	return parts.join(" + ");
}
