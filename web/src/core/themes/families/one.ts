import { defineFamily } from "../palette";

/** Atom One (github.com/atom/one-dark-ui, one-light-ui). */
export default defineFamily({
	id: "one",
	name: "One",
	variants: [
		{
			id: "dark",
			name: "One Dark",
			light: false,
			colors: {
				surface: "#282c34",
				raised: "#21252b",
				sunken: "#2c313c",
				text: "#abb2bf",
				muted: "#5c6370",
				border: "#3e4452",
				accent: "#61afef",
				danger: "#e06c75",
				success: "#98c379",
				warning: "#e5c07b"
			}
		},
		{
			id: "light",
			name: "One Light",
			light: true,
			colors: {
				surface: "#fafafa",
				raised: "#eaeaeb",
				sunken: "#e5e5e6",
				text: "#383a42",
				muted: "#a0a1a7",
				border: "#dbdbdc",
				accent: "#4078f2",
				danger: "#e45649",
				success: "#50a14f",
				warning: "#c18401"
			}
		}
	]
});
