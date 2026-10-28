import type { Client } from "@connectrpc/connect";
import { computed, ref, shallowRef } from "vue";

import { type AppError, toAppError } from "~/core/libs/errors";
import { useRpcClient } from "~/core/libs/rpc";
import { type Entry, EntryKind, FilesService, type ListFolderResponse } from "~/generated/muxmultitool/files/v1/files_pb";

/**
 * One folder of the media roots as the file browser shows it: the breadcrumbs, the subfolders and
 * the media files of the current folder, and wildcard search in it. Shared by the Files screen and
 * the container file-picker dialog. The empty folder id is the roots listing.
 */
export function useFolderBrowser(client: Client<typeof FilesService> = useRpcClient(FilesService)) {
	const folderId = ref("");
	const listing = shallowRef<ListFolderResponse>();
	const loading = ref(false);
	const error = ref<AppError>();
	let latest = 0;

	const folder = computed(() => listing.value?.folder);
	/** Root first, the current folder last; empty at the roots listing. */
	const breadcrumbs = computed<Entry[]>(() => {
		if (!listing.value) {
			return [];
		}
		const trail = [...listing.value.ancestors];
		if (listing.value.folder) {
			trail.push(listing.value.folder);
		}
		return trail;
	});
	const folders = computed(() => listing.value?.entries.filter((entry) => entry.kind === EntryKind.FOLDER) ?? []);
	const files = computed(() => listing.value?.entries.filter((entry) => entry.kind === EntryKind.FILE) ?? []);
	const isRoots = computed(() => folderId.value === "");

	/** Shows `id`, or the roots for an empty id. A newer call wins over an older one still in flight. */
	async function open(id = ""): Promise<void> {
		const call = ++latest;
		folderId.value = id;
		loading.value = true;
		error.value = undefined;
		try {
			const response = await client.listFolder({ folderId: id });
			if (call === latest) {
				listing.value = response;
			}
		} catch (cause) {
			if (call === latest) {
				listing.value = undefined;
				error.value = toAppError(cause);
			}
		} finally {
			if (call === latest) {
				loading.value = false;
			}
		}
	}

	/** The parent folder, or the roots for a root. `undefined` at the roots listing. */
	const parentId = computed(() => (listing.value?.folder ? (listing.value.ancestors.at(-1)?.id ?? "") : undefined));

	function up(): Promise<void> {
		return parentId.value === undefined ? Promise.resolve() : open(parentId.value);
	}

	function reload(): Promise<void> {
		return open(folderId.value);
	}

	/** The media files of the current folder matching any of the wildcard patterns; see `FindFilesRequest`. */
	async function find(patterns: readonly string[], recursive: boolean): Promise<Entry[]> {
		const response = await client.findFiles({ folderId: folderId.value, patterns: [...patterns], recursive });
		return response.files;
	}

	/** The subfolders of any folder, for the tree. Throws an `AppError`. */
	async function subfolders(id: string): Promise<Entry[]> {
		try {
			const response = await client.listFolder({ folderId: id });
			return response.entries.filter((entry) => entry.kind === EntryKind.FOLDER);
		} catch (cause) {
			throw toAppError(cause);
		}
	}

	return { folderId, listing, loading, error, folder, breadcrumbs, folders, files, isRoots, parentId, open, up, reload, find, subfolders };
}

/** Splits the pattern field into patterns: `*.mkv, *.mp4` and `*.mkv *.mp4` both give two. */
export function splitPatterns(input: string): string[] {
	return input
		.split(/[\s,;]+/)
		.map((pattern) => pattern.trim())
		.filter((pattern) => pattern.length > 0);
}

/** Whether a file name has one of the extensions (without the dot, any case). An empty list accepts every name. */
export function hasExtension(name: string, extensions: readonly string[] | undefined): boolean {
	if (!extensions || extensions.length === 0) {
		return true;
	}
	const extension = name.split(".").at(-1)?.toLowerCase();
	return extension !== undefined && extensions.some((candidate) => candidate.toLowerCase() === extension);
}
