import { isDesktop } from "~/core/libs/filePicker";

interface TauriInternals {
	invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>;
}

/**
 * Whether "show in folder" is available: only in the desktop window, where an output is a file on the
 * user's machine. The container offers a download instead (`GET /files/{id}`).
 */
export function canShowInFolder(host: object = globalThis): boolean {
	return isDesktop(host);
}

/**
 * Opens the system file manager with `path` selected, through Tauri's opener plugin (the capability
 * file of the desktop crate allows `reveal_item_in_dir` for the embedded server's origin). Resolves
 * once the plugin answered; rejects with the plugin's message when it could not.
 */
export async function showInFolder(path: string, host: object = globalThis): Promise<void> {
	if (!isDesktop(host)) {
		throw new Error("showing a file in its folder needs the desktop app");
	}
	const internals = (host as Record<string, unknown>).__TAURI_INTERNALS__ as TauriInternals;
	await internals.invoke("plugin:opener|reveal_item_in_dir", { path });
}
