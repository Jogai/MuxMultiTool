import { defineFamily } from "../palette";

/** Nord (nordtheme.com): polar night, snow storm, frost and aurora. One variant; auto mode uses it for light and dark. */
export default defineFamily({
	id: "nord",
	name: "Nord",
	variants: [
		{
			id: "nord",
			name: "Nord",
			light: false,
			colors: {
				surface: "#2e3440",
				raised: "#3b4252",
				sunken: "#434c5e",
				text: "#eceff4",
				muted: "#616e88",
				border: "#4c566a",
				accent: "#88c0d0",
				danger: "#bf616a",
				success: "#a3be8c",
				warning: "#ebcb8b"
			}
		}
	]
});
