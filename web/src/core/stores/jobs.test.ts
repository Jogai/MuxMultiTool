import { Code } from "@connectrpc/connect";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useJobsStore } from "./jobs";
import { createRpcTransport, setRpcTransport } from "~/core/libs/rpc";

describe("useJobsStore", () => {
	beforeEach(() => setActivePinia(createPinia()));
	afterEach(() => setRpcTransport(undefined));

	it("opens the queue stream on the shared transport and reports when the backend is unreachable", async () => {
		const fetch = vi.fn(async (input: string | URL | Request) => {
			const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
			expect(url).toMatch(/^\/rpc\/muxmultitool\.jobs\.v1\.JobsService\/WatchQueue$/);
			throw new TypeError("Failed to fetch");
		});
		setRpcTransport(createRpcTransport({ fetch: fetch as unknown as typeof globalThis.fetch }));

		const store = useJobsStore();
		expect(store.isActive).toBe(false);
		expect(store.running).toBe(0);
		expect(store.queued).toBe(0);

		await vi.waitFor(() => expect(store.connection).toBe("reconnecting"));
		expect(store.connectionError).toMatchObject({ code: Code.Unknown, message: "Failed to fetch" });
		store.stop();
	});
});
