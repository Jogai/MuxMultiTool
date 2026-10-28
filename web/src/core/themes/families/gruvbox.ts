import { defineFamily } from "../palette";

/** Gruvbox (github.com/morhetz/gruvbox), the medium contrast of each side. The hard and soft sides are in Gruvbox Material. */
export default defineFamily({
	id: "gruvbox",
	name: "Gruvbox",
	variants: [
		{
			id: "dark",
			name: "Dark Medium",
			light: false,
			colors: {
				surface: "#282828",
				raised: "#3c3836",
				sunken: "#1d2021",
				text: "#ebdbb2",
				muted: "#a89984",
				border: "#504945",
				accent: "#83a598",
				danger: "#fb4934",
				success: "#b8bb26",
				warning: "#fabd2f"
			}
		},
		{
			id: "light",
			name: "Light Medium",
			light: true,
			colors: {
				surface: "#fbf1c7",
				raised: "#ebdbb2",
				sunken: "#d5c4a1",
				text: "#3c3836",
				muted: "#7c6f64",
				border: "#bdae93",
				accent: "#076678",
				danger: "#9d0006",
				success: "#79740e",
				warning: "#b57614"
			}
		}
	]
});
