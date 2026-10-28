import { defineFamily } from "../palette";

/** Night Owl (github.com/sdras/night-owl-vscode-theme) and Light Owl. */
export default defineFamily({
	id: "night-owl",
	name: "Night Owl",
	variants: [
		{
			id: "night-owl",
			name: "Night Owl",
			light: false,
			colors: {
				surface: "#011627",
				raised: "#0b253a",
				sunken: "#1d3b53",
				text: "#d6deeb",
				muted: "#637777",
				border: "#1d3b53",
				accent: "#82aaff",
				danger: "#ef5350",
				success: "#addb67",
				warning: "#ecc48d"
			}
		},
		{
			id: "light-owl",
			name: "Light Owl",
			light: true,
			colors: {
				surface: "#fbfbfb",
				raised: "#f0f0f0",
				sunken: "#e0e0e0",
				text: "#403f53",
				muted: "#989fb1",
				border: "#d9d9d9",
				accent: "#4876d6",
				danger: "#e64d49",
				success: "#2aa298",
				warning: "#daaa01"
			}
		}
	]
});
