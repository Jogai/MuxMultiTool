import { defineFamily } from "../palette";

/** Kanagawa (github.com/rebelot/kanagawa.nvim): Wave, Dragon and the light Lotus. */
export default defineFamily({
	id: "kanagawa",
	name: "Kanagawa",
	variants: [
		{
			id: "wave",
			name: "Wave",
			light: false,
			colors: {
				surface: "#1f1f28",
				raised: "#16161d",
				sunken: "#2a2a37",
				text: "#dcd7ba",
				muted: "#727169",
				border: "#54546d",
				accent: "#7e9cd8",
				danger: "#e82424",
				success: "#98bb6c",
				warning: "#ff9e3b"
			}
		},
		{
			id: "dragon",
			name: "Dragon",
			light: false,
			colors: {
				surface: "#181616",
				raised: "#12120f",
				sunken: "#282727",
				text: "#c5c9c5",
				muted: "#737c73",
				border: "#625e5a",
				accent: "#8ba4b0",
				danger: "#c4746e",
				success: "#8a9a7b",
				warning: "#b6927b"
			}
		},
		{
			id: "lotus",
			name: "Lotus",
			light: true,
			colors: {
				surface: "#f2ecbc",
				raised: "#e5ddb0",
				sunken: "#dcd5ac",
				text: "#545464",
				muted: "#716e61",
				border: "#d5cea3",
				accent: "#4d699b",
				danger: "#c84053",
				success: "#6f894e",
				warning: "#cc6d00"
			}
		}
	]
});
