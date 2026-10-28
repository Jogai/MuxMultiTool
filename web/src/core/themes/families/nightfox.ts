import { defineFamily } from "../palette";

/** Nightfox (github.com/EdenEast/nightfox.nvim): bg1 is the page, bg0 the bars, bg2 the sunken areas. */
export default defineFamily({
	id: "nightfox",
	name: "Nightfox",
	variants: [
		{
			id: "nightfox",
			name: "Nightfox",
			light: false,
			colors: {
				surface: "#192330",
				raised: "#131a24",
				sunken: "#212e3f",
				text: "#cdcecf",
				muted: "#738091",
				border: "#29394f",
				accent: "#719cd6",
				danger: "#c94f6d",
				success: "#81b29a",
				warning: "#dbc074"
			}
		},
		{
			id: "duskfox",
			name: "Duskfox",
			light: false,
			colors: {
				surface: "#232136",
				raised: "#191726",
				sunken: "#2d2a45",
				text: "#e0def4",
				muted: "#817c9c",
				border: "#373354",
				accent: "#569fba",
				danger: "#eb6f92",
				success: "#a3be8c",
				warning: "#f6c177"
			}
		},
		{
			id: "nordfox",
			name: "Nordfox",
			light: false,
			colors: {
				surface: "#2e3440",
				raised: "#232831",
				sunken: "#39404f",
				text: "#cdcecf",
				muted: "#60728a",
				border: "#444c5e",
				accent: "#81a1c1",
				danger: "#bf616a",
				success: "#a3be8c",
				warning: "#ebcb8b"
			}
		},
		{
			id: "terafox",
			name: "Terafox",
			light: false,
			colors: {
				surface: "#152528",
				raised: "#0f1c1e",
				sunken: "#1d3337",
				text: "#e6eaea",
				muted: "#6d7f8b",
				border: "#254147",
				accent: "#5a93aa",
				danger: "#e85c51",
				success: "#7aa4a1",
				warning: "#fda47f"
			}
		},
		{
			id: "carbonfox",
			name: "Carbonfox",
			light: false,
			colors: {
				surface: "#161616",
				raised: "#0c0c0c",
				sunken: "#252525",
				text: "#f2f4f8",
				muted: "#6e6f70",
				border: "#353535",
				accent: "#78a9ff",
				danger: "#ee5396",
				success: "#25be6a",
				warning: "#08bdba"
			}
		},
		{
			id: "dawnfox",
			name: "Dawnfox",
			light: true,
			colors: {
				surface: "#faf4ed",
				raised: "#ebe5df",
				sunken: "#ebe0df",
				text: "#575279",
				muted: "#9893a5",
				border: "#bdbfc9",
				accent: "#286983",
				danger: "#b4637a",
				success: "#618774",
				warning: "#ea9d34"
			}
		},
		{
			id: "dayfox",
			name: "Dayfox",
			light: true,
			colors: {
				surface: "#f6f2ee",
				raised: "#e4dcd4",
				sunken: "#dbd1dd",
				text: "#3d2b5a",
				muted: "#837a72",
				border: "#d3c7bb",
				accent: "#2848a9",
				danger: "#a5222f",
				success: "#396847",
				warning: "#ac5402"
			}
		}
	]
});
