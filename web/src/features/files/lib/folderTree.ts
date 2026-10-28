import type { AppError } from "~/core/libs/errors";
import type { Entry } from "~/generated/muxmultitool/files/v1/files_pb";

/**
 * One row of the folder tree: a folder, or a note under a folder whose subfolders are not there (yet).
 * The tree is flat for Reka's `TreeRoot`, which asks `childrenOf` for every expanded row.
 */
export type FolderNode =
	| { kind: "folder"; id: string; entry: Entry }
	| { kind: "note"; id: string; note: "loading" | "error" | "empty"; parent: Entry; message?: string };

/** What is known about a folder's subfolders once they were asked for. */
export interface LoadedFolder {
	children?: FolderNode[];
	error?: AppError;
}

export function folderNode(entry: Entry): FolderNode {
	return { kind: "folder", id: entry.id, entry };
}

/**
 * The rows under a folder: its subfolders once loaded, otherwise one note saying why there are none
 * (still loading, failed, or empty). A folder always has children, so it is always expandable; a note
 * never has any.
 */
export function childrenOf(node: FolderNode, loaded: ReadonlyMap<string, LoadedFolder>): FolderNode[] | undefined {
	if (node.kind === "note") {
		return undefined;
	}
	const state = loaded.get(node.id);
	const note = (kind: "loading" | "error" | "empty", message?: string): FolderNode[] => [
		{ kind: "note", id: `note:${kind}:${node.id}`, note: kind, parent: node.entry, message }
	];
	if (!state) {
		return note("loading");
	}
	if (state.error) {
		return note("error", state.error.message);
	}
	return state.children && state.children.length > 0 ? state.children : note("empty");
}
