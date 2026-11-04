import type { ThumbnailSheet } from "~/generated/muxmultitool/media/v1/media_pb";

import type { TimelineView } from "./view";

/** Where a time's picture is in the sprites: the sprite number and the tile's offset inside it. */
export interface Tile {
	index: number;
	sprite: number;
	/** Pixel offset of the tile's top-left corner inside the sprite image. */
	offsetX: number;
	offsetY: number;
}

/** The layout fields the lookup needs; `ThumbnailSheet` has them. */
export type SheetLayout = Pick<ThumbnailSheet, "intervalSeconds" | "tileWidth" | "tileHeight" | "columns" | "rows" | "tileCount">;

/** The tile that shows `seconds`: tile i covers i * interval; a time past the last tile shows the last. */
export function tileAt(sheet: SheetLayout, seconds: number): Tile | undefined {
	const perSprite = sheet.columns * sheet.rows;
	if (!(sheet.intervalSeconds > 0) || perSprite === 0 || sheet.tileCount === 0) {
		return undefined;
	}
	const index = Math.min(Math.max(0, Math.floor(seconds / sheet.intervalSeconds)), sheet.tileCount - 1);
	const inSprite = index % perSprite;
	return {
		index,
		sprite: Math.floor(index / perSprite),
		offsetX: (inSprite % sheet.columns) * sheet.tileWidth,
		offsetY: Math.floor(inSprite / sheet.columns) * sheet.tileHeight
	};
}

/** The sprite URL for a sprite number. */
export function spriteUrl(sheet: Pick<ThumbnailSheet, "spriteUrlTemplate">, sprite: number): string {
	return sheet.spriteUrlTemplate.replace("{n}", String(sprite));
}

/** One picture on the thumbnail lane: a tile drawn `width` pixels wide at `x`. */
export interface ThumbnailSlot {
	x: number;
	width: number;
	seconds: number;
	tile: Tile;
}

/**
 * The pictures to draw over the visible range: one every `slotWidth` pixels (the tile's own width scaled
 * to the lane height, or wider when tiles are further apart than that at the current zoom), each showing
 * the tile of the time at its left edge. Only tiles in a ready sprite are included.
 */
export function thumbnailSlots(view: TimelineView, sheet: SheetLayout, spritesReady: number, laneHeight: number): ThumbnailSlot[] {
	if (!(view.width > 0) || !(sheet.tileHeight > 0) || !(sheet.intervalSeconds > 0)) {
		return [];
	}
	const scale = laneHeight / sheet.tileHeight;
	const naturalWidth = sheet.tileWidth * scale;
	const intervalPx = sheet.intervalSeconds * view.pxPerSecond;
	// Tiles further apart than a picture are drawn at their own positions, with the gaps left to the
	// neighbour; closer ones are thinned out to one per picture width
	const slotWidth = Math.max(naturalWidth, intervalPx);
	const slots: ThumbnailSlot[] = [];
	// Slots are aligned to the content, not the viewport, so they do not jump while scrolling
	const firstX = 0 - ((Math.max(0, view.scroll) * view.pxPerSecond) % slotWidth);
	for (let x = firstX; x < view.width; x += slotWidth) {
		const seconds = Math.max(0, view.scroll + x / view.pxPerSecond);
		if (seconds >= view.duration && view.duration > 0) {
			break;
		}
		const tile = tileAt(sheet, seconds);
		if (!tile || tile.sprite >= spritesReady) {
			continue;
		}
		slots.push({ x, width: slotWidth, seconds, tile });
	}
	return slots;
}
