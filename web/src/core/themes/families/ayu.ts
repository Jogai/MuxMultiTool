import { defineFamily } from "../palette";

/** Ayu (github.com/ayu-theme/ayu-colors): Dark, Mirage and Light. */
export default defineFamily({
	id: "ayu",
	name: "Ayu",
	variants: [
		{
			id: "dark",
			name: "Dark",
			light: false,
			colors: {
				surface: "#0b0e14",
				raised: "#0f131a",
				sunken: "#1b2029",
				text: "#bfbdb6",
				muted: "#565b66",
				border: "#1e232b",
				accent: "#e6b450",
				danger: "#d95757",
				success: "#7fd962",
				warning: "#ff8f40"
			}
		},
		{
			id: "mirage",
			name: "Mirage",
			light: false,
			colors: {
				surface: "#1f2430",
				raised: "#1c212b",
				sunken: "#171b24",
				text: "#cccac2",
				muted: "#707a8c",
				border: "#2d3545",
				accent: "#ffcc66",
				danger: "#ff6666",
				success: "#87d96c",
				warning: "#ffad66"
			}
		},
		{
			id: "light",
			name: "Light",
			light: true,
			colors: {
				surface: "#fcfcfc",
				raised: "#f3f4f5",
				sunken: "#e7eaed",
				text: "#5c6166",
				muted: "#8a9199",
				border: "#d8dbdf",
				accent: "#399ee6",
				danger: "#e65050",
				success: "#6cbf43",
				warning: "#fa8d3e"
			}
		}
	]
});
