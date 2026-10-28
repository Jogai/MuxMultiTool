import { defineStore } from "pinia";

import { useThemeSettings } from "~/core/composables/useThemeSettings";

/** The one theme for the whole app: the top bar toggles it, the settings page edits it. */
export const useThemeStore = defineStore("theme", () => useThemeSettings());
