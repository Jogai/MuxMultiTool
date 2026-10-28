import type { Client } from "@connectrpc/connect";

import type { AppMode, PickedFile } from "~/core/libs/filePicker";
import { type Entry, EntryKind, type FilesService } from "~/generated/muxmultitool/files/v1/files_pb";

/**
 * Turns what the file picker returned into files-service entries, which is what the selection
 * holds. The container browser hands back opaque ids (`GetEntries` looks them up again); native
 * desktop dialogs hand back absolute paths, which only `OpenPaths` may turn into entries. Files
 * that no longer exist are left out.
 */
export async function entriesForPicked(
	client: Client<typeof FilesService>,
	mode: AppMode,
	picked: readonly PickedFile[]
): Promise<Entry[]> {
	if (picked.length === 0) {
		return [];
	}
	const paths = picked.map((file) => file.path);
	const response = mode === "desktop" ? await client.openPaths({ paths }) : await client.getEntries({ ids: paths });
	return response.entries.filter((entry) => entry.kind === EntryKind.FILE);
}

/** What the container browser returns for a chosen entry: the id is the path as the backend knows it. */
export function pickedFromEntry(entry: Entry): PickedFile {
	return { path: entry.id, name: entry.name };
}
