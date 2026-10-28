import { usePreferredDark, useStorage } from "@vueuse/core";
import { computed, watchEffect } from "vue";

/** The two variants of the default palette in `themes/base.css`. */
export type ThemeVariant = "light" | "dark";

/**
 * Light/dark toggle for the default palette, saved per browser.
 * The theme system (TASK-42) replaces this with families and variants; the top bar's toggle keeps working.
 */
export function useTheme() {
	const prefersDark = usePreferredDark();
	const chosen = useStorage<ThemeVariant | "auto">("mmt.theme", "auto");
	const variant = computed<ThemeVariant>(() => (chosen.value === "auto" ? (prefersDark.value ? "dark" : "light") : chosen.value));

	watchEffect(() => {
		document.documentElement.dataset.theme = variant.value;
	});

	function toggle(): void {
		chosen.value = variant.value === "dark" ? "light" : "dark";
	}

	return { variant, toggle };
}
