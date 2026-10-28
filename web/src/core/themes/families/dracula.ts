import { defineFamily } from "../palette";

/** Dracula (draculatheme.com) and its light counterpart Alucard. */
export default defineFamily({
	id: "dracula",
	name: "Dracula",
	variants: [
		{
			id: "dracula",
			name: "Dracula",
			light: false,
			colors: {
				surface: "#282a36",
				raised: "#343746",
				sunken: "#21222c",
				text: "#f8f8f2",
				muted: "#6272a4",
				border: "#44475a",
				accent: "#bd93f9",
				danger: "#ff5555",
				success: "#50fa7b",
				warning: "#ffb86c"
			}
		},
		{
			id: "alucard",
			name: "Alucard",
			light: true,
			colors: {
				surface: "#fffbeb",
				raised: "#f6f2e2",
				sunken: "#cfcfde",
				text: "#1f1f1f",
				muted: "#6c664b",
				border: "#cfcfde",
				accent: "#644ac9",
				danger: "#cb3a2a",
				success: "#14710a",
				warning: "#a34d14"
			}
		}
	]
});
