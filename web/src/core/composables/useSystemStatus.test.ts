import { ConnectError, Code, type Client } from "@connectrpc/connect";
import { create } from "@bufbuild/protobuf";
import { describe, expect, it, vi } from "vitest";

import { useSystemStatus } from "./useSystemStatus";
import {
	GetHealthResponseSchema,
	GetVersionResponseSchema,
	HealthStatus,
	type SystemService
} from "~/generated/muxmultitool/system/v1/system_pb";

function fakeClient(overrides: Partial<Client<typeof SystemService>> = {}): Client<typeof SystemService> {
	return {
		getHealth: vi.fn().mockResolvedValue(create(GetHealthResponseSchema, { status: HealthStatus.OK })),
		getVersion: vi.fn().mockResolvedValue(create(GetVersionResponseSchema, { version: "0.1.0" })),
		...overrides
	};
}

describe("useSystemStatus", () => {
	it("starts loading and reports the version once the backend answers", async () => {
		const status = useSystemStatus(fakeClient());
		expect(status.state.value).toBe("loading");

		await vi.waitFor(() => expect(status.state.value).toBe("ok"));
		expect(status.version.value).toBe("0.1.0");
		expect(status.error.value).toBeUndefined();
	});

	it("reports an error with the Connect message when a call fails", async () => {
		const client = fakeClient({
			getHealth: vi.fn().mockRejectedValue(new ConnectError("backend down", Code.Unavailable))
		});
		const status = useSystemStatus(client);

		await vi.waitFor(() => expect(status.state.value).toBe("error"));
		expect(status.error.value).toBe("[unavailable] backend down");
	});

	it("treats an unspecified health status as an error", async () => {
		const client = fakeClient({
			getHealth: vi.fn().mockResolvedValue(create(GetHealthResponseSchema, { status: HealthStatus.UNSPECIFIED }))
		});
		const status = useSystemStatus(client);

		await vi.waitFor(() => expect(status.state.value).toBe("error"));
	});

	it("refreshes on request", async () => {
		const client = fakeClient();
		const status = useSystemStatus(client);
		await vi.waitFor(() => expect(status.state.value).toBe("ok"));

		await status.refresh();
		expect(client.getHealth).toHaveBeenCalledTimes(2);
		expect(status.state.value).toBe("ok");
	});
});
