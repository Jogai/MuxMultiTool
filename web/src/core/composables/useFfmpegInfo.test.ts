import { create } from "@bufbuild/protobuf";
import type { Client } from "@connectrpc/connect";
import { describe, expect, it, vi } from "vitest";

import { useFfmpegInfo } from "./useFfmpegInfo";
import {
	CapabilitiesSchema,
	CapabilitiesStatus,
	GetCapabilitiesResponseSchema,
	type SystemService
} from "~/generated/muxmultitool/system/v1/system_pb";

function clientWith(getCapabilities: () => Promise<unknown>) {
	return { getCapabilities } as unknown as Client<typeof SystemService>;
}

describe("useFfmpegInfo", () => {
	it("reports the ffmpeg version", async () => {
		const info = useFfmpegInfo(
			clientWith(async () =>
				create(GetCapabilitiesResponseSchema, {
					status: CapabilitiesStatus.AVAILABLE,
					capabilities: create(CapabilitiesSchema, { version: "8.0" })
				})
			)
		);
		await info.load();
		expect(info.state.value).toBe("ok");
		expect(info.version.value).toBe("8.0");
	});

	it("shows a missing ffmpeg as a state", async () => {
		const info = useFfmpegInfo(
			clientWith(async () => create(GetCapabilitiesResponseSchema, { status: CapabilitiesStatus.UNAVAILABLE, error: "not found" }))
		);
		await info.load();
		expect(info.state.value).toBe("unavailable");
		expect(info.reason.value).toBe("not found");
	});

	it("keeps rpc failures as an error", async () => {
		const info = useFfmpegInfo(clientWith(vi.fn().mockRejectedValue(new Error("down"))));
		await info.load();
		expect(info.state.value).toBe("error");
		expect(info.error.value).toBeDefined();
	});
});
