import { describe, expect, it } from "vitest";

import { extensionOf, stem } from "./fileNames";

describe("file names", () => {
	it("split the stem and the extension", () => {
		expect(stem("Film.1.mp4")).toBe("Film.1");
		expect(extensionOf("Film.1.mp4")).toBe("mp4");
		expect(stem("Movie (2024).mkv")).toBe("Movie (2024)");
		expect(extensionOf("movie.tar.gz")).toBe("gz");
		expect(stem("noext")).toBe("noext");
		expect(extensionOf("noext")).toBe("");
		expect(stem(".hidden")).toBe(".hidden");
		expect(extensionOf(".hidden")).toBe("");
	});
});
