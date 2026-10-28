import { describe, expect, it } from "vitest";

import { FAMILIES } from "./index";
import { AA_CONTRAST, contrast, luminance, mix, readable, resolveTokens, TEXT_ON_BACKGROUND, TOKEN_NAMES } from "./palette";

const HEX = /^#[0-9a-f]{6}$/;

describe("colour maths", () => {
	it("computes the WCAG contrast ratio", () => {
		expect(contrast("#000000", "#ffffff")).toBeCloseTo(21, 5);
		expect(contrast("#ffffff", "#000000")).toBeCloseTo(21, 5);
		expect(contrast("#777777", "#777777")).toBe(1);
		// The classic 4.5 boundary: #767676 on white passes, #777777 is just under
		expect(contrast("#767676", "#ffffff")).toBeGreaterThanOrEqual(4.5);
	});

	it("mixes in sRGB", () => {
		expect(mix("#000000", "#ffffff", 0)).toBe("#000000");
		expect(mix("#000000", "#ffffff", 1)).toBe("#ffffff");
		expect(mix("#000000", "#ffffff", 0.5)).toBe("#808080");
	});

	it("leaves a readable colour alone and nudges an unreadable one just enough", () => {
		expect(readable("#1a1a1a", ["#ffffff"], "#000000")).toBe("#1a1a1a");
		const nudged = readable("#6272a4", ["#282a36"], "#ffffff");
		expect(nudged).not.toBe("#6272a4");
		expect(contrast(nudged, "#282a36")).toBeGreaterThanOrEqual(AA_CONTRAST);
		// One step back would not have passed
		expect(contrast(mix(nudged, "#6272a4", 0.02), "#282a36")).toBeLessThan(AA_CONTRAST + 0.1);
	});
});

describe("the shipped theme families", () => {
	it("are the 16 MVP families, each with a unique id, and every variant id unique within its family", () => {
		expect(FAMILIES).toHaveLength(16);
		expect(new Set(FAMILIES.map((family) => family.id)).size).toBe(16);
		for (const family of FAMILIES) {
			expect(family.variants.length).toBeGreaterThan(0);
			for (const id of Object.values(family.defaults ?? {})) {
				expect(
					family.variants.map((variant) => variant.id),
					`${family.id} defaults`
				).toContain(id);
			}
			expect(new Set(family.variants.map((variant) => variant.id)).size).toBe(family.variants.length);
		}
	});

	it("ship the variants the features doc lists", () => {
		const counts = Object.fromEntries(FAMILIES.map((family) => [family.id, family.variants.length]));
		expect(counts).toEqual({
			default: 2,
			dracula: 2,
			nord: 1,
			solarized: 2,
			gruvbox: 2,
			"gruvbox-material": 6,
			catppuccin: 4,
			"tokyo-night": 4,
			"rose-pine": 3,
			one: 2,
			everforest: 6,
			kanagawa: 3,
			"monokai-pro": 6,
			ayu: 3,
			"night-owl": 2,
			nightfox: 7
		});
	});

	for (const family of FAMILIES) {
		for (const variant of family.variants) {
			describe(`${family.name} · ${variant.name}`, () => {
				const tokens = resolveTokens(variant);

				it("gives every token a colour", () => {
					for (const name of TOKEN_NAMES) {
						expect(tokens[name], name).toMatch(name === "overlay" ? /^#[0-9a-f]{8}$/ : HEX);
					}
					for (const colour of Object.values(variant.colors)) {
						expect(colour).toMatch(HEX);
					}
				});

				it("is light or dark as it says", () => {
					const surface = luminance(variant.colors.surface);
					if (variant.light) {
						expect(surface).toBeGreaterThan(0.5);
					} else {
						expect(surface).toBeLessThan(0.15);
					}
				});

				it("meets WCAG AA (4.5:1) on every text/background pair the components use", () => {
					const failures = TEXT_ON_BACKGROUND.flatMap(([foreground, background]) => {
						const ratio = contrast(tokens[foreground], tokens[background]);
						return ratio < AA_CONTRAST ? [`${foreground} on ${background}: ${ratio.toFixed(2)}`] : [];
					});
					expect(failures).toEqual([]);
				});

				it("keeps the surfaces and the border as the palette published them", () => {
					expect(tokens.surface).toBe(variant.colors.surface);
					expect(tokens["surface-raised"]).toBe(variant.colors.raised);
					expect(tokens["surface-sunken"]).toBe(variant.colors.sunken);
					expect(tokens.border).toBe(variant.colors.border);
				});
			});
		}
	}
});
