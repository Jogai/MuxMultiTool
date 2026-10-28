import { computed } from "vue";

import { toggledThemeSetting } from "~/core/libs/theme";
import { useThemeStore } from "~/core/stores/theme";

/** Which side of the family is on screen. */
export type ThemeSide = "light" | "dark";

/**
 * The top bar's light/dark toggle over the theme store: it switches to the other side of the current
 * family as a single variant. A family with one side only (Nord) cannot toggle; the settings page is for that.
 */
export function useTheme() {
	const store = useThemeStore();
	const variant = computed<ThemeSide>(() => (store.resolved.variant.light ? "light" : "dark"));
	const next = computed(() => toggledThemeSetting(store.setting, store.resolved));
	const canToggle = computed(() => next.value !== undefined);
	const familyName = computed(() => store.resolved.family.name);

	function toggle(): void {
		if (next.value) {
			store.update(next.value);
		}
	}

	return { variant, canToggle, familyName, toggle };
}
