import { describe, expect, it } from "vitest";

import { DIGITS, formatShortcut, isDialogTarget, isTypingTarget, type KeyInput, matchShortcut, type Shortcut } from "./shortcuts";

const press = (key: string, modifiers: Partial<KeyInput> = {}): KeyInput => ({
	key,
	shiftKey: false,
	ctrlKey: false,
	metaKey: false,
	altKey: false,
	...modifiers
});

const TABLE: readonly Shortcut<"digit" | "help" | "zoomIn" | "zoomFit" | "letter" | "queue" | "up">[] = [
	{ action: "digit", key: DIGITS },
	{ action: "help", key: "?" },
	{ action: "zoomIn", key: "+" },
	{ action: "zoomFit", key: "0" },
	{ action: "letter", key: "v" },
	{ action: "queue", key: "Enter", ctrl: true },
	{ action: "up", key: "ArrowUp", shift: true }
];

describe("matchShortcut", () => {
	it("matches letters case-insensitively and named keys exactly", () => {
		expect(matchShortcut(TABLE, press("V"))).toEqual({ action: "letter" });
		expect(matchShortcut(TABLE, press("ArrowUp", { shiftKey: true }))).toEqual({ action: "up" });
		expect(matchShortcut(TABLE, press("ArrowUp"))).toBeUndefined();
		expect(matchShortcut(TABLE, press("x"))).toBeUndefined();
	});

	it("takes Ctrl or Cmd for ctrl and never Alt", () => {
		expect(matchShortcut(TABLE, press("Enter", { ctrlKey: true }))).toEqual({ action: "queue" });
		expect(matchShortcut(TABLE, press("Enter", { metaKey: true }))).toEqual({ action: "queue" });
		expect(matchShortcut(TABLE, press("Enter"))).toBeUndefined();
		expect(matchShortcut(TABLE, press("v", { ctrlKey: true }))).toBeUndefined();
		expect(matchShortcut(TABLE, press("v", { altKey: true }))).toBeUndefined();
	});

	it("checks Shift for letters, digits and Space but not for symbols", () => {
		expect(matchShortcut(TABLE, press("v", { shiftKey: true }))).toBeUndefined();
		expect(matchShortcut(TABLE, press("?", { shiftKey: true }))).toEqual({ action: "help" });
		expect(matchShortcut(TABLE, press("+", { shiftKey: true }))).toEqual({ action: "zoomIn" });
		expect(matchShortcut(TABLE, press("7", { shiftKey: true }))).toBeUndefined();
	});

	it("matches a digit with its number, the first row winning", () => {
		expect(matchShortcut(TABLE, press("7"))).toEqual({ action: "digit", digit: 7 });
		expect(matchShortcut(TABLE, press("0"))).toEqual({ action: "digit", digit: 0 });
		expect(
			matchShortcut(
				TABLE.filter((shortcut) => shortcut.key !== DIGITS),
				press("0")
			)
		).toEqual({ action: "zoomFit" });
	});
});

describe("isTypingTarget", () => {
	const element = (tagName: string, isContentEditable = false) => ({ tagName, isContentEditable }) as unknown as EventTarget;

	it("recognises text fields and leaves buttons to the screen", () => {
		expect(isTypingTarget(element("INPUT"))).toBe(true);
		expect(isTypingTarget(element("TEXTAREA"))).toBe(true);
		expect(isTypingTarget(element("SELECT"))).toBe(true);
		expect(isTypingTarget(element("DIV", true))).toBe(true);
		expect(isTypingTarget(element("BUTTON"))).toBe(false);
		expect(isTypingTarget(element("BODY"))).toBe(false);
		expect(isTypingTarget({} as EventTarget)).toBe(false);
		expect(isTypingTarget(null)).toBe(false);
	});
});

describe("isDialogTarget", () => {
	const inside = (role: string) =>
		({ closest: (selector: string) => (selector.includes(`"${role}"`) ? {} : null) }) as unknown as EventTarget;

	it("keeps the screen out of an open dialog", () => {
		expect(isDialogTarget(inside("dialog"))).toBe(true);
		expect(isDialogTarget(inside("alertdialog"))).toBe(true);
		expect(isDialogTarget({ closest: () => null } as unknown as EventTarget)).toBe(false);
		expect(isDialogTarget({} as EventTarget)).toBe(false);
		expect(isDialogTarget(null)).toBe(false);
	});
});

describe("formatShortcut", () => {
	it("spells the keys the way the list shows them", () => {
		expect(formatShortcut({ action: "togglePlay", key: " " })).toBe("Space");
		expect(formatShortcut({ action: "keyframeBack", key: "ArrowLeft", shift: true })).toBe("Shift + ←");
		expect(formatShortcut({ action: "seekForward", key: "ArrowRight", ctrl: true })).toBe("Ctrl + →");
		expect(formatShortcut({ action: "seekForward", key: "ArrowRight", ctrl: true }, { mac: true })).toBe("⌘ + →");
		expect(formatShortcut({ action: "setRangeStart", key: "i" })).toBe("I");
		expect(formatShortcut({ action: "seekStart", key: "Home" })).toBe("Home");
		expect(formatShortcut({ action: "toggleStream", key: DIGITS })).toBe("0 – 9");
		expect(formatShortcut({ action: "queue", key: "Enter", ctrl: true })).toBe("Ctrl + Enter");
		expect(formatShortcut({ action: "redo", key: "z", ctrl: true, shift: true }, { mac: true })).toBe("⌘ + Shift + Z");
	});
});
