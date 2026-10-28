import { describe, expect, it } from "vitest";

import {
	formatShortcut,
	isDialogTarget,
	isTypingTarget,
	type KeyInput,
	PLAYER_SHORTCUTS,
	type PlayerAction,
	shortcutAction
} from "./shortcuts";

function key(key: string, mods: Partial<Omit<KeyInput, "key">> = {}): KeyInput {
	return { key, shiftKey: false, ctrlKey: false, metaKey: false, altKey: false, ...mods };
}

describe("shortcutAction", () => {
	it.each<[string, KeyInput, PlayerAction]>([
		["Space plays and pauses", key(" "), "togglePlay"],
		["← steps a frame back", key("ArrowLeft"), "frameBack"],
		["→ steps a frame forward", key("ArrowRight"), "frameForward"],
		["Shift+← goes to the previous keyframe", key("ArrowLeft", { shiftKey: true }), "keyframeBack"],
		["Shift+→ goes to the next keyframe", key("ArrowRight", { shiftKey: true }), "keyframeForward"],
		["Ctrl+← seeks back", key("ArrowLeft", { ctrlKey: true }), "seekBack"],
		["Ctrl+→ seeks forward", key("ArrowRight", { ctrlKey: true }), "seekForward"],
		["Cmd+→ seeks forward on macOS", key("ArrowRight", { metaKey: true }), "seekForward"],
		["Home goes to the start", key("Home"), "seekStart"],
		["End goes to the end", key("End"), "seekEnd"],
		["I sets the range start", key("i"), "setRangeStart"],
		["I with caps lock still sets the range start", key("I"), "setRangeStart"],
		["O sets the range end", key("o"), "setRangeEnd"],
		["P plays the range", key("p"), "playRange"],
		["L toggles the loop", key("l"), "toggleLoop"],
		["M toggles mute", key("m"), "toggleMute"],
		["? shows the shortcuts, Shift or not", key("?", { shiftKey: true }), "showShortcuts"]
	])("%s", (_label, input, action) => {
		expect(shortcutAction(input)).toBe(action);
	});

	it("ignores keys that are not shortcuts, and Alt combinations", () => {
		expect(shortcutAction(key("x"))).toBeUndefined();
		expect(shortcutAction(key("ArrowUp"))).toBeUndefined();
		expect(shortcutAction(key("ArrowLeft", { altKey: true }))).toBeUndefined();
		expect(shortcutAction(key(" ", { ctrlKey: true }))).toBeUndefined();
		expect(shortcutAction(key("i", { shiftKey: true }))).toBeUndefined();
		expect(shortcutAction(key("ArrowLeft", { shiftKey: true, ctrlKey: true }))).toBeUndefined();
	});

	it("maps every listed shortcut to its own action", () => {
		const seen = new Set<PlayerAction>();
		for (const shortcut of PLAYER_SHORTCUTS) {
			const input = key(shortcut.key, { shiftKey: shortcut.shift ?? false, ctrlKey: shortcut.ctrl ?? false });
			expect(shortcutAction(input)).toBe(shortcut.action);
			seen.add(shortcut.action);
		}
		expect(seen.size).toBe(PLAYER_SHORTCUTS.length);
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
	});
});

describe("isTypingTarget", () => {
	const element = (tagName: string, isContentEditable = false) => ({ tagName, isContentEditable }) as unknown as EventTarget;

	it("recognises text fields and leaves buttons to the player", () => {
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

	it("keeps the player out of an open dialog", () => {
		expect(isDialogTarget(inside("dialog"))).toBe(true);
		expect(isDialogTarget(inside("alertdialog"))).toBe(true);
		expect(isDialogTarget({ closest: () => null } as unknown as EventTarget)).toBe(false);
		expect(isDialogTarget({} as EventTarget)).toBe(false);
		expect(isDialogTarget(null)).toBe(false);
	});
});
