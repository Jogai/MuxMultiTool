import { MethodOptions_IdempotencyLevel } from "@bufbuild/protobuf/wkt";
import { describe, expect, it } from "vitest";

import { SystemService } from "~/generated/muxmultitool/system/v1/system_pb";

describe("SystemService contract", () => {
	it("defines health and version", () => {
		expect(SystemService.methods.map((m) => m.name)).toEqual(["GetHealth", "GetVersion"]);
	});

	it("marks both methods side-effect free so Connect may use GET", () => {
		for (const method of SystemService.methods) {
			expect(method.idempotency).toBe(MethodOptions_IdempotencyLevel.NO_SIDE_EFFECTS);
		}
	});
});
