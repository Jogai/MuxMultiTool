import { defineFamily } from "../palette";

/** The MuxMultiTool palette from `base.css`: zinc greys with a blue accent. */
export default defineFamily({
	id: "default",
	name: "Default",
	variants: [
		{
			id: "dark",
			name: "Dark",
			light: false,
			colors: {
				surface: "#18181b",
				raised: "#27272a",
				sunken: "#09090b",
				text: "#f4f4f5",
				muted: "#a1a1aa",
				border: "#3f3f46",
				accent: "#60a5fa",
				danger: "#f87171",
				success: "#4ade80",
				warning: "#fbbf24"
			}
		},
		{
			id: "light",
			name: "Light",
			light: true,
			colors: {
				surface: "#ffffff",
				raised: "#f4f4f5",
				sunken: "#e4e4e7",
				text: "#1a1a1a",
				muted: "#5f6672",
				border: "#d4d4d8",
				accent: "#2563eb",
				danger: "#b91c1c",
				success: "#15803d",
				warning: "#b45309"
			}
		}
	]
});
