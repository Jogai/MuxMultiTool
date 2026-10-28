import { defineFamily } from "../palette";

/** Catppuccin (catppuccin.com): base, mantle and crust as the surfaces, blue as the accent. */
export default defineFamily({
	id: "catppuccin",
	name: "Catppuccin",
	defaults: { dark: "mocha" },
	variants: [
		{
			id: "latte",
			name: "Latte",
			light: true,
			colors: {
				surface: "#eff1f5",
				raised: "#e6e9ef",
				sunken: "#dce0e8",
				text: "#4c4f69",
				muted: "#6c6f85",
				border: "#ccd0da",
				accent: "#1e66f5",
				danger: "#d20f39",
				success: "#40a02b",
				warning: "#df8e1d"
			}
		},
		{
			id: "frappe",
			name: "Frappé",
			light: false,
			colors: {
				surface: "#303446",
				raised: "#292c3c",
				sunken: "#232634",
				text: "#c6d0f5",
				muted: "#a5adce",
				border: "#414559",
				accent: "#8caaee",
				danger: "#e78284",
				success: "#a6d189",
				warning: "#e5c890"
			}
		},
		{
			id: "macchiato",
			name: "Macchiato",
			light: false,
			colors: {
				surface: "#24273a",
				raised: "#1e2030",
				sunken: "#181926",
				text: "#cad3f5",
				muted: "#a5adcb",
				border: "#363a4f",
				accent: "#8aadf4",
				danger: "#ed8796",
				success: "#a6da95",
				warning: "#eed49f"
			}
		},
		{
			id: "mocha",
			name: "Mocha",
			light: false,
			colors: {
				surface: "#1e1e2e",
				raised: "#181825",
				sunken: "#11111b",
				text: "#cdd6f4",
				muted: "#a6adc8",
				border: "#313244",
				accent: "#89b4fa",
				danger: "#f38ba8",
				success: "#a6e3a1",
				warning: "#f9e2af"
			}
		}
	]
});
