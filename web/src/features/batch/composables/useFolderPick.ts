import type { Client } from "@connectrpc/connect";

import { useFilePicker } from "~/core/composables/useFilePicker";
import type { FilePicker } from "~/core/libs/filePicker";
import { folderForPicked } from "~/core/libs/pickedFiles";
import { useRpcClient } from "~/core/libs/rpc";
import { type Entry, FilesService } from "~/generated/muxmultitool/files/v1/files_pb";

/**
 * Picks a folder for the Batch screen: the in-app browser in the container, the native dialog on
 * desktop, answered as the folder's files-service entry so the screen can show its path and send
 * its id.
 */
export function useFolderPick(client: Client<typeof FilesService> = useRpcClient(FilesService), picker: FilePicker = useFilePicker()) {
	/** Resolves with the chosen folder, or with nothing when the user cancelled. */
	async function pickFolder(startIn?: string): Promise<Entry | undefined> {
		const [picked] = await picker.pickFiles({ directory: true, multiple: false, startIn });
		return folderForPicked(client, picker.mode, picked);
	}

	return { pickFolder };
}
