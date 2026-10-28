import { useIntervalFn, useNow } from "@vueuse/core";

/** The current time, updated once a second: enough for elapsed times, and far cheaper than `useNow`'s per-frame default. */
export function useClock() {
	return useNow({ scheduler: (tick) => useIntervalFn(tick, 1000) });
}
