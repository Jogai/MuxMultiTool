import { defineStore } from "pinia";

import { useSystemStatus } from "~/core/composables/useSystemStatus";

/** One backend health check shared by the status bar and the settings page. */
export const useSystemStore = defineStore("system", () => useSystemStatus());
