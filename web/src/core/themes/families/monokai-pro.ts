import { defineFamily } from "../palette";

/** Monokai Pro (monokai.pro) filters, yellow as the accent. */
export default defineFamily({
	id: "monokai-pro",
	name: "Monokai Pro",
	variants: [
		{
			id: "classic",
			name: "Classic",
			light: false,
			colors: {
				surface: "#2d2a2e",
				raised: "#403e41",
				sunken: "#221f22",
				text: "#fcfcfa",
				muted: "#939293",
				border: "#5b595c",
				accent: "#ffd866",
				danger: "#ff6188",
				success: "#a9dc76",
				warning: "#fc9867"
			}
		},
		{
			id: "machine",
			name: "Machine",
			light: false,
			colors: {
				surface: "#273136",
				raised: "#3a4449",
				sunken: "#1d2528",
				text: "#f2fffc",
				muted: "#8b9798",
				border: "#545f62",
				accent: "#ffed72",
				danger: "#ff6d7e",
				success: "#a2e57b",
				warning: "#ffb270"
			}
		},
		{
			id: "octagon",
			name: "Octagon",
			light: false,
			colors: {
				surface: "#282a3a",
				raised: "#3a3d4b",
				sunken: "#1e1f2b",
				text: "#eaf2f1",
				muted: "#888d94",
				border: "#535763",
				accent: "#ffd76d",
				danger: "#ff657a",
				success: "#bad761",
				warning: "#ff9b5e"
			}
		},
		{
			id: "ristretto",
			name: "Ristretto",
			light: false,
			colors: {
				surface: "#2c2525",
				raised: "#403838",
				sunken: "#211c1c",
				text: "#fff1f3",
				muted: "#948a8b",
				border: "#5b5353",
				accent: "#f9cc6c",
				danger: "#fd6883",
				success: "#adda78",
				warning: "#f38d70"
			}
		},
		{
			id: "spectrum",
			name: "Spectrum",
			light: false,
			colors: {
				surface: "#222222",
				raised: "#363537",
				sunken: "#191919",
				text: "#f7f1ff",
				muted: "#8b888f",
				border: "#525053",
				accent: "#fce566",
				danger: "#fc618d",
				success: "#7bd88f",
				warning: "#fd9353"
			}
		},
		{
			id: "light",
			name: "Light",
			light: true,
			colors: {
				surface: "#faf4f2",
				raised: "#f1ece9",
				sunken: "#e8e3e0",
				text: "#29242a",
				muted: "#706b6e",
				border: "#d1cdcd",
				accent: "#cc7a0a",
				danger: "#e14775",
				success: "#269d69",
				warning: "#e16032"
			}
		}
	]
});
