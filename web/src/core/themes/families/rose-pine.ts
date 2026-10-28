import { defineFamily } from "../palette";

/** Rosé Pine (rosepinetheme.com): rose as the accent, love, foam and gold for the states. */
export default defineFamily({
	id: "rose-pine",
	name: "Rosé Pine",
	variants: [
		{
			id: "main",
			name: "Rosé Pine",
			light: false,
			colors: {
				surface: "#191724",
				raised: "#1f1d2e",
				sunken: "#26233a",
				text: "#e0def4",
				muted: "#908caa",
				border: "#403d52",
				accent: "#ebbcba",
				danger: "#eb6f92",
				success: "#9ccfd8",
				warning: "#f6c177"
			}
		},
		{
			id: "moon",
			name: "Moon",
			light: false,
			colors: {
				surface: "#232136",
				raised: "#2a273f",
				sunken: "#393552",
				text: "#e0def4",
				muted: "#908caa",
				border: "#44415a",
				accent: "#ea9a97",
				danger: "#eb6f92",
				success: "#9ccfd8",
				warning: "#f6c177"
			}
		},
		{
			id: "dawn",
			name: "Dawn",
			light: true,
			colors: {
				surface: "#faf4ed",
				raised: "#fffaf3",
				sunken: "#f2e9e1",
				text: "#575279",
				muted: "#797593",
				border: "#dfdad9",
				accent: "#d7827e",
				danger: "#b4637a",
				success: "#56949f",
				warning: "#ea9d34"
			}
		}
	]
});
