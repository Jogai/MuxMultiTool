import { defineStore } from "pinia";

import { useJobQueue } from "~/core/composables/useJobQueue";

/**
 * The live job queue, shared by the top bar's indicator, the Jobs screen and whatever submits jobs.
 * One `WatchQueue` stream for the whole app, opened on first use and resumed after every disconnect.
 */
export const useJobsStore = defineStore("jobs", () => useJobQueue());
