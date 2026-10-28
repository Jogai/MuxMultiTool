import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it } from "vitest";

import { useJobsStore } from "./jobs";

describe("useJobsStore", () => {
	beforeEach(() => setActivePinia(createPinia()));

	it("is idle until the queue reports work", () => {
		const store = useJobsStore();
		expect(store.isActive).toBe(false);

		store.setCounts({ running: 2, queued: 3 });
		expect(store.isActive).toBe(true);
		expect(store.running).toBe(2);
		expect(store.queued).toBe(3);
	});
});
