import { describe, expect, it } from "vitest";

import {
	applyThemeTokens,
	defaultThemeSetting,
	normalizeThemeSetting,
	parseStoredTheme,
	resolveTheme,
	type ThemeRoot,
	type ThemeSetting,
	themeSettingFromProto,
	themeSettingToProto,
	toggledThemeSetting
} from "./theme";
import { resolveTokens, TOKEN_NAMES } from "~/core/themes/palette";
import { ThemeMode } from "~/generated/muxmultitool/system/v1/system_pb";

const catppuccinAuto: ThemeSetting = { family: "catppuccin", mode: "auto", variant: "mocha", lightVariant: "latte", darkVariant: "mocha" };

describe("normalizeThemeSetting", () => {
	it("gives the default palette in auto mode when nothing is stored", () => {
		expect(normalizeThemeSetting(undefined)).toEqual(defaultThemeSetting());
		expect(defaultThemeSetting()).toEqual({ family: "default", mode: "auto", variant: "dark", lightVariant: "light", darkVariant: "dark" });
	});

	it("keeps a valid setting as it is", () => {
		expect(normalizeThemeSetting(catppuccinAuto)).toEqual(catppuccinAuto);
	});

	it("falls back to the default family for an unknown one", () => {
		expect(normalizeThemeSetting({ family: "material", mode: "single", variant: "ocean" })).toEqual({
			...defaultThemeSetting(),
			mode: "single"
		});
	});

	it("resets variants that are not in the family to the family's light and dark defaults", () => {
		expect(
			normalizeThemeSetting({ family: "catppuccin", mode: "auto", variant: "nord", lightVariant: "dark", darkVariant: "light" })
		).toEqual({
			family: "catppuccin",
			mode: "auto",
			variant: "mocha",
			lightVariant: "latte",
			darkVariant: "mocha"
		});
	});

	it("uses the one variant for both sides of a family without a light one", () => {
		expect(normalizeThemeSetting({ family: "nord" })).toEqual({
			family: "nord",
			mode: "auto",
			variant: "nord",
			lightVariant: "nord",
			darkVariant: "nord"
		});
	});

	it("treats a missing or unknown mode as auto", () => {
		expect(normalizeThemeSetting({ family: "dracula" }).mode).toBe("auto");
		expect(normalizeThemeSetting({ family: "dracula", mode: "weird" as ThemeSetting["mode"] }).mode).toBe("auto");
	});
});

describe("resolveTheme", () => {
	it("follows the OS in auto mode", () => {
		expect(resolveTheme(catppuccinAuto, true).variant.id).toBe("mocha");
		expect(resolveTheme(catppuccinAuto, false).variant.id).toBe("latte");
		expect(resolveTheme(catppuccinAuto, false).family.id).toBe("catppuccin");
	});

	it("ignores the OS in single mode", () => {
		const single: ThemeSetting = { ...catppuccinAuto, mode: "single", variant: "macchiato" };
		expect(resolveTheme(single, true).variant.id).toBe("macchiato");
		expect(resolveTheme(single, false).variant.id).toBe("macchiato");
	});

	it("resolves a Nord auto setting to Nord either way", () => {
		const nord = normalizeThemeSetting({ family: "nord" });
		expect(resolveTheme(nord, true).variant.id).toBe("nord");
		expect(resolveTheme(nord, false).variant.id).toBe("nord");
	});

	it("never fails on a broken setting", () => {
		const resolved = resolveTheme({ family: "", mode: "single", variant: "", lightVariant: "", darkVariant: "" }, true);
		expect(resolved.family.id).toBe("default");
		expect(resolved.variant.id).toBe("dark");
	});
});

describe("toggledThemeSetting", () => {
	it("switches to the other side of the family as a single variant", () => {
		const dark = resolveTheme(catppuccinAuto, true);
		expect(toggledThemeSetting(catppuccinAuto, dark)).toEqual({ ...catppuccinAuto, mode: "single", variant: "latte" });
		const light = resolveTheme(catppuccinAuto, false);
		expect(toggledThemeSetting(catppuccinAuto, light)).toEqual({ ...catppuccinAuto, mode: "single", variant: "mocha" });
	});

	it("has nothing to toggle to in a family with one side", () => {
		const nord = normalizeThemeSetting({ family: "nord" });
		expect(toggledThemeSetting(nord, resolveTheme(nord, true))).toBeUndefined();
	});
});

describe("parseStoredTheme", () => {
	it("reads what was stored and shrugs at anything else", () => {
		expect(parseStoredTheme(JSON.stringify(catppuccinAuto))).toEqual(catppuccinAuto);
		expect(parseStoredTheme(null)).toBeUndefined();
		expect(parseStoredTheme("")).toBeUndefined();
		expect(parseStoredTheme("not json")).toBeUndefined();
		expect(parseStoredTheme('"a string"')).toBeUndefined();
	});
});

describe("the proto mapping", () => {
	it("round-trips through the system service message", () => {
		const message = themeSettingToProto(catppuccinAuto);
		expect(message.mode).toBe(ThemeMode.AUTO);
		expect(normalizeThemeSetting(themeSettingFromProto(message))).toEqual(catppuccinAuto);

		const single = themeSettingToProto({ ...catppuccinAuto, mode: "single" });
		expect(single.mode).toBe(ThemeMode.SINGLE);
		expect(themeSettingFromProto(single).mode).toBe("single");
	});

	it("reads an unspecified mode as single, like the server does", () => {
		const message = themeSettingToProto(catppuccinAuto);
		message.mode = ThemeMode.UNSPECIFIED;
		expect(themeSettingFromProto(message).mode).toBe("single");
	});
});

describe("applyThemeTokens", () => {
	it("writes every token, the colour scheme and the variant name to the root", () => {
		const properties = new Map<string, string>();
		const root: ThemeRoot = { style: { setProperty: (name, value) => properties.set(name, value) }, dataset: {} };
		const resolved = resolveTheme(catppuccinAuto, false);
		applyThemeTokens(root, resolved, resolveTokens(resolved.variant));

		for (const name of TOKEN_NAMES) {
			expect(properties.get(`--mmt-${name}`)).toMatch(/^#[0-9a-f]{6,8}$/);
		}
		expect(properties.get("--mmt-surface")).toBe("#eff1f5");
		expect(properties.get("color-scheme")).toBe("light");
		expect(root.dataset["theme"]).toBe("catppuccin/latte");
	});
});
