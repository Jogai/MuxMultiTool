import { describe, expect, it } from "vitest";

import { type KeyInput, matchShortcut } from "~/core/libs/shortcuts";

import { COMBINE_SHORTCUTS, STREAMS_SHORTCUTS } from "./shortcuts";

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
});
