import { defineFamily } from "../palette";

/** Tokyo Night (github.com/folke/tokyonight.nvim): Night, Storm, Moon and the light Day. */
export default defineFamily({
	id: "tokyo-night",
	name: "Tokyo Night",
	variants: [
		{
			id: "night",
			name: "Night",
			light: false,
			colors: {
				surface: "#1a1b26",
				raised: "#16161e",
				sunken: "#292e42",
				text: "#c0caf5",
				muted: "#565f89",
				border: "#3b4261",
				accent: "#7aa2f7",
				danger: "#f7768e",
				success: "#9ece6a",
				warning: "#e0af68"
			}
		},
		{
			id: "storm",
			name: "Storm",
			light: false,
			colors: {
				surface: "#24283b",
				raised: "#1f2335",
				sunken: "#292e42",
				text: "#c0caf5",
				muted: "#565f89",
				border: "#3b4261",
				accent: "#7aa2f7",
				danger: "#f7768e",
				success: "#9ece6a",
				warning: "#e0af68"
			}
		},
		{
			id: "moon",
			name: "Moon",
			light: false,
			colors: {
				surface: "#222436",
				raised: "#1e2030",
				sunken: "#2f334d",
				text: "#c8d3f5",
				muted: "#636da6",
				border: "#3b4261",
				accent: "#82aaff",
				danger: "#ff757f",
				success: "#c3e88d",
				warning: "#ffc777"
			}
		},
		{
			id: "day",
			name: "Day",
			light: true,
			colors: {
				surface: "#e1e2e7",
				raised: "#d0d5e3",
				sunken: "#c4c8da",
				text: "#3760bf",
				muted: "#6172b0",
				border: "#b4b5b9",
				accent: "#2e7de9",
				danger: "#f52a65",
				success: "#587539",
				warning: "#8c6c3e"
			}
		}
	]
});
