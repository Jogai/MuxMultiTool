import { create } from "@bufbuild/protobuf";

import { DEFAULT_FAMILY, defaultDarkVariant, defaultLightVariant, findFamily, findVariant } from "~/core/themes";
import type { ThemeFamily, ThemeTokens, ThemeVariant } from "~/core/themes/palette";
import {
	ThemeMode as ProtoThemeMode,
	type ThemeSetting as ProtoThemeSetting,
	ThemeSettingSchema
} from "~/generated/muxmultitool/system/v1/system_pb";

/** One variant, or a light and a dark one that follow the OS setting. */
export type ThemeMode = "single" | "auto";

/** The theme choice as stored: on the server through the system service, and in the browser as a fallback. */
export interface ThemeSetting {
	family: string;
	mode: ThemeMode;
	/** The variant in `single` mode. */
	variant: string;
	/** The variant in `auto` mode when the OS is light. */
	lightVariant: string;
	/** The variant in `auto` mode when the OS is dark. */
	darkVariant: string;
}

/** A setting resolved against the palettes and the OS: what is on screen. */
export interface ResolvedTheme {
	family: ThemeFamily;
	variant: ThemeVariant;
}

/** Where the browser keeps its copy of the setting. */
export const THEME_STORAGE_KEY = "mmt.theme";

/** The default palette, following the OS. */
export function defaultThemeSetting(): ThemeSetting {
	return {
		family: DEFAULT_FAMILY.id,
		mode: "auto",
		variant: defaultDarkVariant(DEFAULT_FAMILY).id,
		lightVariant: defaultLightVariant(DEFAULT_FAMILY).id,
		darkVariant: defaultDarkVariant(DEFAULT_FAMILY).id
	};
}

/**
 * A complete, valid setting from whatever was stored: an unknown family becomes the default family,
 * a variant that is not in the family becomes the family's default for that role. So a palette that is
 * renamed or removed never leaves the app without a theme.
 */
export function normalizeThemeSetting(raw: Partial<ThemeSetting> | null | undefined): ThemeSetting {
	if (!raw) {
		return defaultThemeSetting();
	}
	const family = findFamily(raw.family) ?? DEFAULT_FAMILY;
	const dark = defaultDarkVariant(family);
	const light = defaultLightVariant(family);
	return {
		family: family.id,
		mode: raw.mode === "single" ? "single" : "auto",
		variant: (findVariant(family, raw.variant) ?? dark).id,
		lightVariant: (findVariant(family, raw.lightVariant) ?? light).id,
		darkVariant: (findVariant(family, raw.darkVariant) ?? dark).id
	};
}

/** The variant to show for a setting, given what the OS prefers. */
export function resolveTheme(setting: ThemeSetting, prefersDark: boolean): ResolvedTheme {
	const normalized = normalizeThemeSetting(setting);
	const family = findFamily(normalized.family) ?? DEFAULT_FAMILY;
	const id = normalized.mode === "auto" ? (prefersDark ? normalized.darkVariant : normalized.lightVariant) : normalized.variant;
	return { family, variant: findVariant(family, id) ?? defaultDarkVariant(family) };
}

/**
 * The setting the top bar's toggle switches to: the family's other side as a single variant, or
 * `undefined` for a family that has no other side (Nord), where the toggle is disabled.
 */
export function toggledThemeSetting(setting: ThemeSetting, current: ResolvedTheme): ThemeSetting | undefined {
	const { family, variant } = current;
	const other = variant.light ? defaultDarkVariant(family) : defaultLightVariant(family);
	if (other.id === variant.id || other.light === variant.light) {
		return undefined;
	}
	return { ...setting, mode: "single", variant: other.id };
}

/** The browser's copy of the setting, or `undefined` when there is none or it is not JSON. */
export function parseStoredTheme(json: string | null | undefined): Partial<ThemeSetting> | undefined {
	if (!json) {
		return undefined;
	}
	try {
		const parsed: unknown = JSON.parse(json);
		return typeof parsed === "object" && parsed !== null ? (parsed as Partial<ThemeSetting>) : undefined;
	} catch {
		return undefined;
	}
}

export function themeSettingToProto(setting: ThemeSetting): ProtoThemeSetting {
	return create(ThemeSettingSchema, {
		family: setting.family,
		mode: setting.mode === "auto" ? ProtoThemeMode.AUTO : ProtoThemeMode.SINGLE,
		variant: setting.variant,
		lightVariant: setting.lightVariant,
		darkVariant: setting.darkVariant
	});
}

/** The server's theme as a partial setting; `normalizeThemeSetting` completes it. */
export function themeSettingFromProto(message: ProtoThemeSetting): Partial<ThemeSetting> {
	return {
		family: message.family,
		mode: message.mode === ProtoThemeMode.AUTO ? "auto" : "single",
		variant: message.variant,
		lightVariant: message.lightVariant,
		darkVariant: message.darkVariant
	};
}

/** The part of `document.documentElement` the theme writes to; tests pass a plain object. */
export interface ThemeRoot {
	style: { setProperty(name: string, value: string): void };
	dataset: Record<string, string | undefined>;
}

/** Puts the tokens on the root element, where `base.css` reads them, and names the variant for devtools and CSS. */
export function applyThemeTokens(root: ThemeRoot, resolved: ResolvedTheme, tokens: ThemeTokens): void {
	for (const [name, value] of Object.entries(tokens)) {
		root.style.setProperty(`--mmt-${name}`, value);
	}
	root.style.setProperty("color-scheme", resolved.variant.light ? "light" : "dark");
	root.dataset["theme"] = `${resolved.family.id}/${resolved.variant.id}`;
}
