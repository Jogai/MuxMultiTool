import { isDesktop } from "~/core/libs/filePicker";

/** Where the project lives and where the license text is published (architecture.md, "Decisions – License"). */
export const REPOSITORY_URL = "https://github.com/muxmultitool/muxmultitool";
export const LICENSE_ID = "GPL-3.0-or-later";
export const LICENSE_URL = "https://www.gnu.org/licenses/gpl-3.0.html";

interface TauriInternals {
	invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>;
}

/**
 * Handles a click on an external link. In a browser the anchor's own `target="_blank"` does the job and this
 * returns false. In the desktop window a new window would open inside the app, so the URL goes to the system
 * browser through Tauri's opener plugin command instead and this returns true (the caller prevents the default).
 */
export function openExternal(url: string, host: object = globalThis): boolean {
	if (!isDesktop(host)) return false;
	const internals = (host as Record<string, unknown>).__TAURI_INTERNALS__ as TauriInternals;
	void internals.invoke("plugin:opener|open_url", { url });
	return true;
}
