/**
 * The fixed keyboard shortcuts of the screens (mvp-features.md, "Fixed keyboard shortcuts"). Each screen
 * keeps its own declarative table (`features/<screen>/lib/shortcuts.ts`), which feeds both the matcher
 * and the list opened with `?`; this is the matching and the spelling they share.
 */

/** One row of a screen's mapping. */
export interface Shortcut<A extends string = string> {
	action: A;
	/** `KeyboardEvent.key`; letters are matched case-insensitively. `DIGITS` stands for any digit. */
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

/** The `key` of a shortcut on any digit; the match carries the digit. */
export const DIGITS = "0-9";

/** The parts of a keyboard event the mapping looks at. */
export type KeyInput = Pick<KeyboardEvent, "key" | "shiftKey" | "ctrlKey" | "metaKey" | "altKey">;

function sameKey(shortcut: Shortcut, key: string): boolean {
	if (shortcut.key === DIGITS) {
		return /^\d$/.test(key);
	}
	return shortcut.key.length === 1 ? shortcut.key.toLowerCase() === key.toLowerCase() : shortcut.key === key;
}

/**
 * The shortcut a key press maps to in `shortcuts`, or `undefined` when it is none of them. Alt never
 * takes part (it is the browser's). `?` and `+` are typed with Shift on most layouts, so for shortcuts
 * on a symbol the Shift state is not checked; letters, digits and Space do check it.
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
		const symbol = shortcut.key.length === 1 && !/[a-z0-9 ]/i.test(shortcut.key);
		if (symbol || (shortcut.shift ?? false) === event.shiftKey) {
			return shortcut.key === DIGITS ? { action: shortcut.action, digit: Number(event.key) } : { action: shortcut.action };
		}
	}
	return undefined;
}

const TYPING_TAGS = new Set(["INPUT", "TEXTAREA", "SELECT"]);

/** Whether the key press belongs to a text field or a control that handles keys itself, where the screen must stay out of the way. */
export function isTypingTarget(target: EventTarget | null): boolean {
	// Structural, not `instanceof HTMLElement`, so it works for any element and without a DOM in tests
	const element = target as { tagName?: unknown; isContentEditable?: unknown } | null;
	if (!element || typeof element.tagName !== "string") {
		return false;
	}
	return element.isContentEditable === true || TYPING_TAGS.has(element.tagName);
}

const DIALOG_SELECTOR = '[role="dialog"], [role="alertdialog"]';

/**
 * Whether the key press comes from inside a dialog, which has the keyboard to itself: a dialog over
 * the screen (the cut editor's split dialog, say) must not play or step frames behind its back.
 */
export function isDialogTarget(target: EventTarget | null): boolean {
	const element = target as { closest?: unknown } | null;
	if (!element || typeof element.closest !== "function") {
		return false;
	}
	return (element.closest as (selector: string) => unknown)(DIALOG_SELECTOR) != null;
}

const KEY_LABELS: Record<string, string> = {
	[DIGITS]: "0 – 9",
	" ": "Space",
	ArrowLeft: "←",
	ArrowRight: "→",
	ArrowUp: "↑",
	ArrowDown: "↓"
};

/** The shortcut as the list shows it, e.g. `Shift + →` or `Ctrl + Enter`. */
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
