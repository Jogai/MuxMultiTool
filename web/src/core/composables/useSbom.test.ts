import { describe, expect, it } from "vitest";

import { useSbom } from "./useSbom";

function fetchWith(status: number, body: unknown, statusText = ""): typeof fetch {
	return async () =>
		new Response(typeof body === "string" ? body : JSON.stringify(body), {
			status,
			statusText,
			headers: { "content-type": "application/vnd.cyclonedx+json" }
		});
}

const document = {
	bomFormat: "CycloneDX",
	metadata: { component: { name: "muxmultitool", version: "0.1.0" } },
	components: [{ type: "library", name: "tokio", version: "1.54.1", purl: "pkg:cargo/tokio@1.54.1", licenses: [{ expression: "MIT" }] }]
};

describe("useSbom", () => {
	it("loads and parses the document", async () => {
		const sbom = useSbom(fetchWith(200, document));
		expect(sbom.state.value).toBe("idle");
		await sbom.load();
		expect(sbom.state.value).toBe("ok");
		expect(sbom.sbom.value?.components).toHaveLength(1);
		expect(sbom.sbom.value?.application?.version).toBe("0.1.0");
	});

	it("reports an HTTP error", async () => {
		const sbom = useSbom(fetchWith(503, "", "Service Unavailable"));
		await sbom.load();
		expect(sbom.state.value).toBe("error");
		expect(sbom.error.value?.message).toBe("503 Service Unavailable");
	});

	it("reports a body that is not a bill of materials", async () => {
		const sbom = useSbom(fetchWith(200, { hello: "world" }));
		await sbom.load();
		expect(sbom.state.value).toBe("error");
		expect(sbom.error.value?.message).toBe("not a CycloneDX document");
	});

	it("reports a failed fetch", async () => {
		const sbom = useSbom(async () => {
			throw new TypeError("Failed to fetch");
		});
		await sbom.load();
		expect(sbom.state.value).toBe("error");
		expect(sbom.error.value?.message).toBe("Failed to fetch");
	});
});
