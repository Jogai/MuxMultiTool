/**
 * The theme families that ship. Adding a family is a palette file in `families/` plus one line here,
 * in the order the settings screen shows them.
 */
import ayu from "./families/ayu";
import catppuccin from "./families/catppuccin";
import defaultFamily from "./families/default";
import dracula from "./families/dracula";
import everforest from "./families/everforest";
import gruvbox from "./families/gruvbox";
import gruvboxMaterial from "./families/gruvbox-material";
import kanagawa from "./families/kanagawa";
import monokaiPro from "./families/monokai-pro";
import nightOwl from "./families/night-owl";
import nightfox from "./families/nightfox";
import nord from "./families/nord";
import one from "./families/one";
import rosePine from "./families/rose-pine";
import solarized from "./families/solarized";
import tokyoNight from "./families/tokyo-night";
import type { ThemeFamily, ThemeVariant } from "./palette";

export const FAMILIES: readonly ThemeFamily[] = [
	defaultFamily,
	dracula,
	nord,
	solarized,
	gruvbox,
	gruvboxMaterial,
	catppuccin,
	tokyoNight,
	rosePine,
	one,
	everforest,
	kanagawa,
	monokaiPro,
	ayu,
	nightOwl,
	nightfox
];

/** What an unknown or missing setting falls back to. */
export const DEFAULT_FAMILY: ThemeFamily = defaultFamily;

export function findFamily(id: string | undefined): ThemeFamily | undefined {
	return FAMILIES.find((family) => family.id === id);
}

export function findVariant(family: ThemeFamily, id: string | undefined): ThemeVariant | undefined {
	return family.variants.find((variant) => variant.id === id);
}

/** The variant that previews a family and that auto mode uses at night: the family's named default, else the first dark one, else the first. */
export function defaultDarkVariant(family: ThemeFamily): ThemeVariant {
	const variant = findVariant(family, family.defaults?.dark) ?? family.variants.find((candidate) => !candidate.light) ?? family.variants[0];
	if (!variant) {
		throw new Error(`theme family ${family.id} has no variants`);
	}
	return variant;
}

/** The variant auto mode uses by day: the family's named default, else the first light one, or the dark default for a family without one (Nord). */
export function defaultLightVariant(family: ThemeFamily): ThemeVariant {
	return findVariant(family, family.defaults?.light) ?? family.variants.find((candidate) => candidate.light) ?? defaultDarkVariant(family);
}
