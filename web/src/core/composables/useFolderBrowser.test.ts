import { create } from "@bufbuild/protobuf";
import { Code, ConnectError } from "@connectrpc/connect";
import { describe, expect, it, vi } from "vitest";

import { hasExtension, splitPatterns, useFolderBrowser } from "./useFolderBrowser";
import { EntryKind, FindFilesResponseSchema } from "~/generated/muxmultitool/files/v1/files_pb";
import { fakeFilesClient, moviesListing, rootsListing } from "~/test/fakeFilesClient";

const roots = rootsListing;
const movies = moviesListing;

describe("useFolderBrowser", () => {
	it("opens the roots and then a folder with its breadcrumbs", async () => {
		const browser = useFolderBrowser(fakeFilesClient());
		await browser.open();
		expect(browser.isRoots.value).toBe(true);
		expect(browser.breadcrumbs.value).toEqual([]);
		expect(browser.folders.value.map((entry) => entry.name)).toEqual(["media"]);
		expect(browser.parentId.value).toBeUndefined();

		await browser.open("movies");
		expect(browser.breadcrumbs.value.map((entry) => entry.name)).toEqual(["media", "Movies"]);
		expect(browser.folders.value.map((entry) => entry.id)).toEqual(["y2024"]);
		expect(browser.files.value.map((entry) => entry.id)).toEqual(["a"]);
		expect(browser.parentId.value).toBe("root");
		expect(browser.loading.value).toBe(false);
	});

	it("reports a mapped error and keeps nothing stale", async () => {
		const client = fakeFilesClient({ listFolder: vi.fn().mockRejectedValue(new ConnectError("gone", Code.NotFound)) });
		const browser = useFolderBrowser(client);
		await browser.open("missing");
		expect(browser.error.value?.codeName).toBe("not_found");
		expect(browser.listing.value).toBeUndefined();
	});

	it("lets the newest call win", async () => {
		let resolveSlow: (value: typeof roots) => void = () => {};
		const slow = new Promise<typeof roots>((resolve) => {
			resolveSlow = resolve;
		});
		const client = fakeFilesClient({
			listFolder: vi.fn().mockImplementation(({ folderId }: { folderId: string }) => (folderId === "slow" ? slow : Promise.resolve(movies)))
		});
		const browser = useFolderBrowser(client);
		const first = browser.open("slow");
		await browser.open("movies");
		resolveSlow(roots);
		await first;
		expect(browser.folderId.value).toBe("movies");
		expect(browser.folder.value?.name).toBe("Movies");
	});

	it("finds files in the current folder", async () => {
		const client = fakeFilesClient({
			findFiles: vi.fn().mockResolvedValue(create(FindFilesResponseSchema, { files: [{ id: "a", name: "a.mkv", kind: EntryKind.FILE }] }))
		});
		const browser = useFolderBrowser(client);
		await browser.open("movies");
		const found = await browser.find(["*.mkv"], true);
		expect(found.map((entry) => entry.id)).toEqual(["a"]);
		expect(client.findFiles).toHaveBeenCalledWith({ folderId: "movies", patterns: ["*.mkv"], recursive: true });
	});

	it("lists the subfolders of any folder for the tree", async () => {
		const browser = useFolderBrowser(fakeFilesClient());
		expect((await browser.subfolders("movies")).map((entry) => entry.name)).toEqual(["2024"]);
	});
});

describe("splitPatterns", () => {
	it("accepts spaces, commas and semicolons", () => {
		expect(splitPatterns(" *.mkv, *.mp4 ;*.webm ")).toEqual(["*.mkv", "*.mp4", "*.webm"]);
		expect(splitPatterns("")).toEqual([]);
	});
});

describe("hasExtension", () => {
	it("matches case-insensitively and accepts everything without a filter", () => {
		expect(hasExtension("Movie.MKV", ["mkv"])).toBe(true);
		expect(hasExtension("Movie.mp4", ["mkv"])).toBe(false);
		expect(hasExtension("Movie.mp4", [])).toBe(true);
		expect(hasExtension("Movie.mp4", undefined)).toBe(true);
	});
});
