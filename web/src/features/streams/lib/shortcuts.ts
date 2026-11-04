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

export interface Shortcut<A extends string> {
	action: A;
	/** `KeyboardEvent.key`; letters are matched case-insensitively. `0-9` stands for any digit. */
	key: string;
	shift?: boolean;
	/** Ctrl, or Cmd on macOS. */
	ctrl?: boolean;
}

/** A matched shortcut: the action and, for the digit keys, the digit that was pressed. */
export interface ShortcutMatch<A extends string> {
	action: A;
	digit?: number;
}

export const DIGITS = "0-9";

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

/** The parts of a keyboard event the mapping looks at. */
export type KeyInput = Pick<KeyboardEvent, "key" | "shiftKey" | "ctrlKey" | "metaKey" | "altKey">;

function sameKey(shortcut: Shortcut<string>, key: string): boolean {
	if (shortcut.key === DIGITS) {
		return /^\d$/.test(key);
	}
	return shortcut.key.length === 1 ? shortcut.key.toLowerCase() === key.toLowerCase() : shortcut.key === key;
}

/**
 * The shortcut a key press maps to in `shortcuts`, or `undefined` when it is none of them. Alt never
 * takes part (it is the browser's). A `?` is typed with Shift on most layouts, so for shortcuts on a
 * symbol the Shift state is not checked.
 */
export function matchShortcut<A extends string>(shortcuts: readonly Shortcut<A>[], event: KeyInput): ShortcutMatch<A> | undefined {
	if (event.altKey) {
		return undefined;
	}
	const ctrl = event.ctrlKey || event.metaKey;
	for (const shortcut of shortcuts) {
		if (!sameKey(shortcut, event.key) || (shortcut.ctrl ?? false) !== ctrl) {
			continue;
		}
		const symbol = shortcut.key.length === 1 && !/[a-z ]/i.test(shortcut.key);
		if (symbol || (shortcut.shift ?? false) === event.shiftKey) {
			return shortcut.key === DIGITS ? { action: shortcut.action, digit: Number(event.key) } : { action: shortcut.action };
		}
	}
	return undefined;
}

/** Whether the key press belongs to a text field or a control that handles keys itself, where the screen must stay out of the way. */
export function isTypingTarget(target: EventTarget | null): boolean {
	// Structural, not `instanceof HTMLElement`, so it works for any element and without a DOM in tests
	const element = target as { tagName?: unknown; isContentEditable?: unknown } | null;
	if (!element || typeof element.tagName !== "string") {
		return false;
	}
	return element.isContentEditable === true || TYPING_TAGS.has(element.tagName);
}

const TYPING_TAGS = new Set(["INPUT", "TEXTAREA", "SELECT"]);

const KEY_LABELS: Record<string, string> = {
	[DIGITS]: "0 – 9",
	" ": "Space",
	ArrowLeft: "←",
	ArrowRight: "→",
	ArrowUp: "↑",
	ArrowDown: "↓"
};

/** The shortcut as the list shows it, e.g. `Shift + ↑` or `Ctrl + Enter`. */
export function formatShortcut(shortcut: Shortcut<string>, platform: { mac: boolean } = { mac: false }): string {
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
