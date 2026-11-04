import { describe, expect, it } from "vitest";

import { type SheetLayout, spriteUrl, thumbnailSlots, tileAt } from "./thumbnails";

const sheet: SheetLayout = { intervalSeconds: 10, tileWidth: 160, tileHeight: 90, columns: 5, rows: 2, tileCount: 25 };

describe("thumbnail tiles", () => {
	it("finds the sprite and offset of a time", () => {
		expect(tileAt(sheet, 0)).toEqual({ index: 0, sprite: 0, offsetX: 0, offsetY: 0 });
		expect(tileAt(sheet, 75)).toEqual({ index: 7, sprite: 0, offsetX: 320, offsetY: 90 });
		expect(tileAt(sheet, 100)).toEqual({ index: 10, sprite: 1, offsetX: 0, offsetY: 0 });
		expect(tileAt(sheet, 10_000)).toEqual({ index: 24, sprite: 2, offsetX: 640, offsetY: 0 });
		expect(tileAt(sheet, -1)?.index).toBe(0);
		expect(tileAt({ ...sheet, tileCount: 0 }, 5)).toBeUndefined();
	});

	it("fills the sprite number into the URL template", () => {
		expect(spriteUrl({ spriteUrlTemplate: "/media/abc/thumbs/{n}.jpg" }, 3)).toBe("/media/abc/thumbs/3.jpg");
	});

	it("lays out one picture per tile width when zoomed out", () => {
		const view = { duration: 250, width: 1000, pxPerSecond: 4, scroll: 0 }; // a tile every 40 px, a picture is 80 px wide at 45 px high
		const slots = thumbnailSlots(view, sheet, 3, 45);
		expect(slots.map((slot) => slot.x)).toEqual([0, 80, 160, 240, 320, 400, 480, 560, 640, 720, 800, 880, 960]);
		expect(slots[1]).toMatchObject({ width: 80, seconds: 20, tile: { index: 2 } });
	});

	it("spaces the pictures by the interval when zoomed in and aligns them to the content", () => {
		const view = { duration: 250, width: 1000, pxPerSecond: 20, scroll: 7 }; // a tile every 200 px
		const slots = thumbnailSlots(view, sheet, 3, 45);
		expect(slots.map((slot) => slot.x)).toEqual([-140, 60, 260, 460, 660, 860]);
		expect(slots[0]?.tile.index).toBe(0);
		expect(slots[1]?.tile.index).toBe(1);
	});

	it("leaves out tiles whose sprite is not ready", () => {
		const view = { duration: 250, width: 1000, pxPerSecond: 4, scroll: 0 };
		expect(thumbnailSlots(view, sheet, 1, 45).every((slot) => slot.tile.sprite === 0)).toBe(true);
		expect(thumbnailSlots(view, sheet, 0, 45)).toEqual([]);
	});
});
