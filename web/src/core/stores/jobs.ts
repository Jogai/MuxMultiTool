import { defineStore } from "pinia";
import { computed, ref } from "vue";

/**
 * Queue counts for the top bar's job indicator.
 * The job queue UI (TASK-17) feeds them from the `WatchQueue` stream; until then they stay at zero.
 */
export const useJobsStore = defineStore("jobs", () => {
	const running = ref(0);
	const queued = ref(0);
	const isActive = computed(() => running.value > 0 || queued.value > 0);

	function setCounts(counts: { running: number; queued: number }): void {
		running.value = counts.running;
		queued.value = counts.queued;
	}

	return { running, queued, isActive, setCounts };
});
