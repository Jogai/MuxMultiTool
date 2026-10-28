import { create } from "@bufbuild/protobuf";
import type { Client } from "@connectrpc/connect";
import { vi } from "vitest";

import {
	EntryKind,
	type FilesService,
	FindFilesResponseSchema,
	GetEntriesResponseSchema,
	GetStreamDetailsResponseSchema,
	GetStreamReportResponseSchema,
	ListFolderResponseSchema,
	OpenPathsResponseSchema
} from "~/generated/muxmultitool/files/v1/files_pb";

/** The roots listing: one media root. */
export const rootsListing = create(ListFolderResponseSchema, {
	entries: [{ id: "root", name: "media", kind: EntryKind.FOLDER }]
});

/** The `Movies` folder under the root: one subfolder and one file. */
export const moviesListing = create(ListFolderResponseSchema, {
	folder: { id: "movies", name: "Movies", kind: EntryKind.FOLDER, path: "Movies" },
	ancestors: [{ id: "root", name: "media", kind: EntryKind.FOLDER }],
	entries: [
		{ id: "y2024", name: "2024", kind: EntryKind.FOLDER, path: "Movies/2024" },
		{ id: "a", name: "a.mkv", kind: EntryKind.FILE, path: "Movies/a.mkv", sizeBytes: 10n }
	]
});

/** A files-service client whose every method answers with an empty response unless overridden. */
export function fakeFilesClient(overrides: Partial<Client<typeof FilesService>> = {}): Client<typeof FilesService> {
	return {
		listFolder: vi
			.fn()
			.mockImplementation(({ folderId }: { folderId: string }) => Promise.resolve(folderId === "" ? rootsListing : moviesListing)),
		findFiles: vi.fn().mockResolvedValue(create(FindFilesResponseSchema)),
		getEntries: vi.fn().mockResolvedValue(create(GetEntriesResponseSchema)),
		openPaths: vi.fn().mockResolvedValue(create(OpenPathsResponseSchema)),
		getStreamReport: vi.fn().mockResolvedValue(create(GetStreamReportResponseSchema)),
		getStreamDetails: vi.fn().mockResolvedValue(create(GetStreamDetailsResponseSchema)),
		...overrides
	};
}
