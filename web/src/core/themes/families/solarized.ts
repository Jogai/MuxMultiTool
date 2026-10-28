import { defineFamily } from "../palette";

/** Solarized (ethanschoonover.com/solarized): the base tones flip between dark and light, the accents stay. */
export default defineFamily({
	id: "solarized",
	name: "Solarized",
	variants: [
		{
			id: "dark",
			name: "Solarized Dark",
			light: false,
			colors: {
				surface: "#002b36",
				raised: "#073642",
				sunken: "#00212b",
				text: "#93a1a1",
				muted: "#839496",
				border: "#586e75",
				accent: "#268bd2",
				danger: "#dc322f",
				success: "#859900",
				warning: "#b58900"
			}
		},
		{
			id: "light",
			name: "Solarized Light",
			light: true,
			colors: {
				surface: "#fdf6e3",
				raised: "#eee8d5",
				sunken: "#e4dcc3",
				text: "#586e75",
				muted: "#657b83",
				border: "#93a1a1",
				accent: "#268bd2",
				danger: "#dc322f",
				success: "#859900",
				warning: "#b58900"
			}
		}
	]
});
