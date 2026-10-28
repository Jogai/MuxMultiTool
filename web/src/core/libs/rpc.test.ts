import { MethodOptions_IdempotencyLevel } from "@bufbuild/protobuf/wkt";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createRpcTransport, getRpcTransport, setRpcTransport, useRpcClient } from "./rpc";
import { SystemService } from "~/generated/muxmultitool/system/v1/system_pb";

/** A fetch that records every request and answers each unary call with an empty JSON message. */
function recordingFetch() {
	const requests: { url: string; method: string }[] = [];
	const fetch = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
		const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
		requests.push({ url, method: init?.method ?? "GET" });
		return new Response("{}", { status: 200, headers: { "content-type": "application/json" } });
	});
	return { requests, fetch: fetch as unknown as typeof globalThis.fetch };
}

afterEach(() => setRpcTransport(undefined));

describe("SystemService contract", () => {
	it("defines health, version, capabilities and the settings pair", () => {
		expect(SystemService.methods.map((m) => m.name)).toEqual([
			"GetHealth",
			"GetVersion",
			"GetCapabilities",
			"GetSettings",
			"UpdateSettings"
		]);
	});

	it("marks every read side-effect free so Connect may use GET, and the update not", () => {
		for (const method of SystemService.methods) {
			const expected =
				method.name === "UpdateSettings"
					? MethodOptions_IdempotencyLevel.IDEMPOTENCY_UNKNOWN
					: MethodOptions_IdempotencyLevel.NO_SIDE_EFFECTS;
			expect(method.idempotency, method.name).toBe(expected);
		}
	});
});

describe("createRpcTransport", () => {
	it("calls /rpc on the page's own origin, with no host configured anywhere", async () => {
		const { requests, fetch } = recordingFetch();
		setRpcTransport(createRpcTransport({ fetch }));

		await useRpcClient(SystemService).getVersion({});

		expect(requests).toHaveLength(1);
		expect(requests[0]?.url).toMatch(/^\/rpc\/muxmultitool\.system\.v1\.SystemService\/GetVersion\b/);
	});

	it("sends side-effect-free methods as GET requests", async () => {
		const { requests, fetch } = recordingFetch();
		setRpcTransport(createRpcTransport({ fetch }));

		await useRpcClient(SystemService).getHealth({});

		expect(requests[0]?.method).toBe("GET");
	});

	it("accepts another mount point", async () => {
		const { requests, fetch } = recordingFetch();
		setRpcTransport(createRpcTransport({ baseUrl: "/api/", fetch }));

		await useRpcClient(SystemService).getHealth({});

		expect(requests[0]?.url).toMatch(/^\/api\/muxmultitool\.system\.v1\.SystemService\/GetHealth\b/);
	});
});

describe("useRpcClient", () => {
	it("creates the shared transport once and one client per service", () => {
		const first = useRpcClient(SystemService);
		expect(useRpcClient(SystemService)).toBe(first);
		expect(getRpcTransport()).toBe(getRpcTransport());
	});

	it("drops the cached clients when the transport is replaced", () => {
		const before = useRpcClient(SystemService);
		setRpcTransport(createRpcTransport({ fetch: recordingFetch().fetch }));
		expect(useRpcClient(SystemService)).not.toBe(before);
	});
});
