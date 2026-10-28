import { describe, expect, it } from "vitest";

import { formatBuildInfo, readBuildInfo } from "./buildInfo";

function fakeDoc(metas: Record<string, string>) {
	return {
		querySelector: (selector: string) => {
			const name = /name="([^"]+)"/.exec(selector)?.[1] ?? "";
			return name in metas ? { getAttribute: () => metas[name] } : null;
		}
	} as unknown as Document;
}

describe("buildInfo", () => {
	it("reads the plugin's meta tags", () => {
		const info = readBuildInfo("0.1.0", fakeDoc({ "git-commitHash": "abc123", "git-branch": "master", "build-time": "now" }));
		expect(info).toEqual({ version: "0.1.0", commit: "abc123", branch: "master", tag: undefined, buildTime: "now" });
	});

	it("formats the copyable block and skips what is missing", () => {
		const info = readBuildInfo("0.1.0", fakeDoc({ "git-commitHash": "abc123", "git-branch": "master" }));
		expect(formatBuildInfo(info, "8.0")).toBe("MuxMultiTool 0.1.0\ncommit abc123 (master)\nffmpeg 8.0");
		expect(formatBuildInfo({ version: "0.1.0" })).toBe("MuxMultiTool 0.1.0");
	});
});
