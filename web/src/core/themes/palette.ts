/**
 * What a theme family is made of, and how its official colours become the `--mmt-*` tokens.
 *
 * A palette file (`families/*.ts`) gives every variant ten official colours. The semantic tokens that
 * `base.css` maps to Tailwind (`bg-surface`, `text-muted`, …) are derived from them here, at runtime:
 * tints for the accent and danger surfaces, and text colours nudged towards black or white until they
 * reach WCAG AA (4.5:1) on every background they are used on. The nudge stops at the first step that
 * passes, so a palette whose colours already pass stays exactly as published.
 */

/** The ten colours a variant provides, as `#rrggbb`. */
export interface PaletteColors {
	/** Page background. */
	surface: string;
	/** Bars, cards, dialogs and menus. */
	raised: string;
	/** Code blocks and hover backgrounds. */
	sunken: string;
	/** Body text. */
	text: string;
	/** Secondary text: labels, hints, inactive navigation. */
	muted: string;
	/** Lines between regions. */
	border: string;
	/** Links, the active navigation entry, primary buttons. */
	accent: string;
	danger: string;
	success: string;
	warning: string;
}

export interface ThemeVariant {
	/** Stable id, stored in the settings, e.g. `mocha`. */
	id: string;
	/** Display name as the family spells it, e.g. `Mocha`. Proper nouns are not translated. */
	name: string;
	/** Whether this is a light variant: the sun candidate in auto mode. */
	light: boolean;
	colors: PaletteColors;
}

export interface ThemeFamily {
	/** Stable id, stored in the settings, e.g. `catppuccin`. */
	id: string;
	name: string;
	/** In the order the family lists them. */
	variants: readonly ThemeVariant[];
	/**
	 * The variants auto mode starts with and the family card previews, when they are not the first dark
	 * and the first light one in the list (Catppuccin leads with Latte but Mocha is its face).
	 */
	defaults?: { dark?: string; light?: string };
}

/** Identity helper so palette files get type checking and completion without an import of the types. */
export function defineFamily(family: ThemeFamily): ThemeFamily {
	return family;
}

/** The semantic tokens, without the `--mmt-` prefix. Each has a `--color-*` twin in `base.css`. */
export const TOKEN_NAMES = [
	"surface",
	"surface-raised",
	"surface-sunken",
	"text",
	"muted",
	"border",
	"accent",
	"on-accent",
	"accent-surface",
	"danger",
	"danger-surface",
	"success",
	"warning",
	"overlay"
] as const;

export type TokenName = (typeof TOKEN_NAMES)[number];
export type ThemeTokens = Record<TokenName, string>;

/** The contrast WCAG AA asks for normal text. */
export const AA_CONTRAST = 4.5;

type Rgb = [number, number, number];

const HEX = /^#([0-9a-f]{6})$/i;

export function parseHex(color: string): Rgb {
	const match = HEX.exec(color);
	if (!match) {
		throw new Error(`not a #rrggbb colour: ${color}`);
	}
	const value = Number.parseInt(match[1] ?? "", 16);
	return [(value >> 16) & 0xff, (value >> 8) & 0xff, value & 0xff];
}

export function toHex([r, g, b]: Rgb): string {
	return `#${[r, g, b].map((channel) => Math.round(channel).toString(16).padStart(2, "0")).join("")}`;
}

function channelLuminance(channel: number): number {
	const c = channel / 255;
	return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
}

/** Relative luminance per WCAG 2. */
export function luminance(color: string): number {
	const [r, g, b] = parseHex(color);
	return 0.2126 * channelLuminance(r) + 0.7152 * channelLuminance(g) + 0.0722 * channelLuminance(b);
}

/** Contrast ratio per WCAG 2, between 1 and 21. */
export function contrast(a: string, b: string): number {
	const la = luminance(a);
	const lb = luminance(b);
	const [light, dark] = la > lb ? [la, lb] : [lb, la];
	return (light + 0.05) / (dark + 0.05);
}

/** `amount` of `b` mixed into `a`, in sRGB: 0 is `a`, 1 is `b`. */
export function mix(a: string, b: string, amount: number): string {
	const from = parseHex(a);
	const to = parseHex(b);
	return toHex([0, 1, 2].map((i) => (from[i] ?? 0) + ((to[i] ?? 0) - (from[i] ?? 0)) * amount) as Rgb);
}

/**
 * `color`, moved towards `ink` in steps of 1% until it reaches `minimum` contrast on every background.
 * Returns `color` unchanged when it already passes.
 */
export function readable(color: string, backgrounds: readonly string[], ink: string, minimum = AA_CONTRAST): string {
	for (let step = 0; step <= 100; step++) {
		const candidate = mix(color, ink, step / 100);
		if (backgrounds.every((background) => contrast(candidate, background) >= minimum)) {
			return candidate;
		}
	}
	return ink;
}

/** The `--mmt-*` tokens for a variant. */
export function resolveTokens(variant: ThemeVariant): ThemeTokens {
	const { colors, light } = variant;
	const ink = light ? "#000000" : "#ffffff";
	const surfaces = [colors.surface, colors.raised, colors.sunken];
	const accentSurface = mix(colors.accent, colors.surface, 0.82);
	const dangerSurface = mix(colors.danger, colors.surface, 0.85);
	const accent = readable(colors.accent, [colors.surface, colors.raised, accentSurface], ink);
	const danger = readable(colors.danger, [colors.surface, colors.raised, dangerSurface], ink);
	// The darker of the deepest background and the text, half transparent: a scrim that dims either way.
	const scrim = luminance(colors.sunken) < luminance(colors.text) ? colors.sunken : colors.text;
	return {
		surface: colors.surface,
		"surface-raised": colors.raised,
		"surface-sunken": colors.sunken,
		text: readable(colors.text, surfaces, ink),
		muted: readable(colors.muted, surfaces, ink),
		border: colors.border,
		accent,
		// The accent passes against the surface, so the surface passes against the accent.
		"on-accent": colors.surface,
		"accent-surface": accentSurface,
		danger,
		"danger-surface": dangerSurface,
		success: readable(colors.success, [colors.surface, colors.raised], ink),
		warning: readable(colors.warning, [colors.surface, colors.raised], ink),
		overlay: `${scrim}80`
	};
}

/**
 * Every foreground/background pair the components put together, as token names.
 * The contrast test checks each of them for every variant; keep it in step with how the tokens are used.
 */
export const TEXT_ON_BACKGROUND: readonly (readonly [TokenName, TokenName])[] = [
	["text", "surface"],
	["text", "surface-raised"],
	["text", "surface-sunken"],
	["muted", "surface"],
	["muted", "surface-raised"],
	["muted", "surface-sunken"],
	["accent", "surface"],
	["accent", "surface-raised"],
	["accent", "accent-surface"],
	["on-accent", "accent"],
	["danger", "surface"],
	["danger", "surface-raised"],
	["danger", "danger-surface"],
	["success", "surface"],
	["success", "surface-raised"],
	["warning", "surface"],
	["warning", "surface-raised"]
];
