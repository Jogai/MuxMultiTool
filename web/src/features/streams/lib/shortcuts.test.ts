import { describe, expect, it } from "vitest";

import { COMBINE_SHORTCUTS, formatShortcut, isTypingTarget, type KeyInput, matchShortcut, STREAMS_SHORTCUTS } from "./shortcuts";

const press = (key: string, modifiers: Partial<KeyInput> = {}): KeyInput => ({
	key,
	shiftKey: false,
	ctrlKey: false,
	metaKey: false,
	altKey: false,
	...modifiers
});

describe("shortcuts", () => {
	it("maps the Streams keys, digits with their number", () => {
		expect(matchShortcut(STREAMS_SHORTCUTS, press("7"))).toEqual({ action: "toggleStream", digit: 7 });
		expect(matchShortcut(STREAMS_SHORTCUTS, press("["))).toEqual({ action: "previousFile" });
		expect(matchShortcut(STREAMS_SHORTCUTS, press("V"))).toEqual({ action: "toggleVideoOnly" });
		expect(matchShortcut(STREAMS_SHORTCUTS, press("Enter", { ctrlKey: true }))).toEqual({ action: "queue" });
		expect(matchShortcut(STREAMS_SHORTCUTS, press("Enter", { metaKey: true }))).toEqual({ action: "queue" });
		expect(matchShortcut(STREAMS_SHORTCUTS, press("?", { shiftKey: true }))).toEqual({ action: "showShortcuts" });
		expect(matchShortcut(STREAMS_SHORTCUTS, press("Enter"))).toBeUndefined();
		expect(matchShortcut(STREAMS_SHORTCUTS, press("v", { shiftKey: true }))).toBeUndefined();
		expect(matchShortcut(STREAMS_SHORTCUTS, press("v", { altKey: true }))).toBeUndefined();
	});

	it("maps the Combine keys", () => {
		expect(matchShortcut(COMBINE_SHORTCUTS, press("ArrowUp", { shiftKey: true }))).toEqual({ action: "trackUp" });
		expect(matchShortcut(COMBINE_SHORTCUTS, press("ArrowUp"))).toBeUndefined();
		expect(matchShortcut(COMBINE_SHORTCUTS, press("Delete"))).toEqual({ action: "removeTrack" });
		expect(matchShortcut(COMBINE_SHORTCUTS, press("7"))).toBeUndefined();
	});

	it("stays out of text fields", () => {
		expect(isTypingTarget({ tagName: "INPUT" } as unknown as EventTarget)).toBe(true);
		expect(isTypingTarget({ tagName: "DIV", isContentEditable: true } as unknown as EventTarget)).toBe(true);
		expect(isTypingTarget({ tagName: "BUTTON" } as unknown as EventTarget)).toBe(false);
		expect(isTypingTarget(null)).toBe(false);
	});

	it("formats the keys for the list", () => {
		expect(formatShortcut({ action: "toggleStream", key: "0-9" })).toBe("0 – 9");
		expect(formatShortcut({ action: "queue", key: "Enter", ctrl: true })).toBe("Ctrl + Enter");
		expect(formatShortcut({ action: "queue", key: "Enter", ctrl: true }, { mac: true })).toBe("⌘ + Enter");
		expect(formatShortcut({ action: "trackUp", key: "ArrowUp", shift: true })).toBe("Shift + ↑");
	});
});
