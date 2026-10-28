import { defineFamily } from "../palette";

/** Gruvbox Material (github.com/sainnhe/gruvbox-material), material foreground, each contrast level. */
export default defineFamily({
	id: "gruvbox-material",
	name: "Gruvbox Material",
	defaults: { dark: "dark-medium", light: "light-medium" },
	variants: [
		{
			id: "dark-hard",
			name: "Dark Hard",
			light: false,
			colors: {
				surface: "#1d2021",
				raised: "#282828",
				sunken: "#141617",
				text: "#d4be98",
				muted: "#928374",
				border: "#3c3836",
				accent: "#7daea3",
				danger: "#ea6962",
				success: "#a9b665",
				warning: "#d8a657"
			}
		},
		{
			id: "dark-medium",
			name: "Dark Medium",
			light: false,
			colors: {
				surface: "#282828",
				raised: "#32302f",
				sunken: "#1b1b1b",
				text: "#d4be98",
				muted: "#928374",
				border: "#45403d",
				accent: "#7daea3",
				danger: "#ea6962",
				success: "#a9b665",
				warning: "#d8a657"
			}
		},
		{
			id: "dark-soft",
			name: "Dark Soft",
			light: false,
			colors: {
				surface: "#32302f",
				raised: "#3c3836",
				sunken: "#252423",
				text: "#d4be98",
				muted: "#928374",
				border: "#504945",
				accent: "#7daea3",
				danger: "#ea6962",
				success: "#a9b665",
				warning: "#d8a657"
			}
		},
		{
			id: "light-hard",
			name: "Light Hard",
			light: true,
			colors: {
				surface: "#f9f5d7",
				raised: "#f5edca",
				sunken: "#f2e5bc",
				text: "#654735",
				muted: "#928374",
				border: "#ebdbb2",
				accent: "#45707a",
				danger: "#c14a4a",
				success: "#6c782e",
				warning: "#b47109"
			}
		},
		{
			id: "light-medium",
			name: "Light Medium",
			light: true,
			colors: {
				surface: "#fbf1c7",
				raised: "#f4e8be",
				sunken: "#eee0b7",
				text: "#654735",
				muted: "#928374",
				border: "#ddccab",
				accent: "#45707a",
				danger: "#c14a4a",
				success: "#6c782e",
				warning: "#b47109"
			}
		},
		{
			id: "light-soft",
			name: "Light Soft",
			light: true,
			colors: {
				surface: "#f2e5bc",
				raised: "#eddeb5",
				sunken: "#e6d5ae",
				text: "#654735",
				muted: "#928374",
				border: "#d5c4a1",
				accent: "#45707a",
				danger: "#c14a4a",
				success: "#6c782e",
				warning: "#b47109"
			}
		}
	]
});
