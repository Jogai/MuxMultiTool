import { defineFamily } from "../palette";

/** Everforest (github.com/sainnhe/everforest), each contrast level; blue as the accent so green stays the success colour. */
export default defineFamily({
	id: "everforest",
	name: "Everforest",
	defaults: { dark: "dark-medium", light: "light-medium" },
	variants: [
		{
			id: "dark-hard",
			name: "Dark Hard",
			light: false,
			colors: {
				surface: "#272e33",
				raised: "#2e383c",
				sunken: "#1e2326",
				text: "#d3c6aa",
				muted: "#859289",
				border: "#414b50",
				accent: "#7fbbb3",
				danger: "#e67e80",
				success: "#a7c080",
				warning: "#dbbc7f"
			}
		},
		{
			id: "dark-medium",
			name: "Dark Medium",
			light: false,
			colors: {
				surface: "#2d353b",
				raised: "#343f44",
				sunken: "#232a2e",
				text: "#d3c6aa",
				muted: "#859289",
				border: "#475258",
				accent: "#7fbbb3",
				danger: "#e67e80",
				success: "#a7c080",
				warning: "#dbbc7f"
			}
		},
		{
			id: "dark-soft",
			name: "Dark Soft",
			light: false,
			colors: {
				surface: "#333c43",
				raised: "#3a464c",
				sunken: "#293136",
				text: "#d3c6aa",
				muted: "#859289",
				border: "#4d5960",
				accent: "#7fbbb3",
				danger: "#e67e80",
				success: "#a7c080",
				warning: "#dbbc7f"
			}
		},
		{
			id: "light-hard",
			name: "Light Hard",
			light: true,
			colors: {
				surface: "#fffbef",
				raised: "#f8f5e4",
				sunken: "#f2efdf",
				text: "#5c6a72",
				muted: "#939f91",
				border: "#e8e5d5",
				accent: "#3a94c5",
				danger: "#f85552",
				success: "#8da101",
				warning: "#dfa000"
			}
		},
		{
			id: "light-medium",
			name: "Light Medium",
			light: true,
			colors: {
				surface: "#fdf6e3",
				raised: "#f4f0d9",
				sunken: "#efebd4",
				text: "#5c6a72",
				muted: "#939f91",
				border: "#e0dcc7",
				accent: "#3a94c5",
				danger: "#f85552",
				success: "#8da101",
				warning: "#dfa000"
			}
		},
		{
			id: "light-soft",
			name: "Light Soft",
			light: true,
			colors: {
				surface: "#f3ead3",
				raised: "#eae4ca",
				sunken: "#e5dfc5",
				text: "#5c6a72",
				muted: "#939f91",
				border: "#d8d3ba",
				accent: "#3a94c5",
				danger: "#f85552",
				success: "#8da101",
				warning: "#dfa000"
			}
		}
	]
});
