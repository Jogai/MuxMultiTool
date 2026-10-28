/**
 * File picking differs per mode: the RPC file browser in the container, native dialogs on desktop.
 * This module is the only place in the frontend that knows about Tauri (architecture.md, "Desktop specifics").
 */

/** How the frontend is being served. */
export type AppMode = "container" | "desktop";

export interface PickFilesOptions {
	/** Allow more than one file. Default: true. */
	multiple?: boolean;
	/** Pick a folder instead of files. Default: false. */
	directory?: boolean;
	/** Folder to start in: a path under a media root in the container, any path on desktop. */
	startIn?: string;
	/** Accepted extensions without the dot, e.g. `["mkv", "mp4"]`. Empty or absent means all files. */
	extensions?: string[];
}

export interface PickedFile {
	/** The path as the backend knows it: from the RPC browser in the container, from the native dialog on desktop. */
	path: string;
	/** Display name, the last path segment. */
	name: string;
}

export interface FilePicker {
	readonly mode: AppMode;
	/** Resolves with the chosen files; an empty array when the user cancelled. */
	pickFiles(options?: PickFilesOptions): Promise<PickedFile[]>;
}

/** What the container picker delegates to: the in-app file browser dialog. */
export type OpenFileBrowser = (options: PickFilesOptions) => Promise<PickedFile[]>;

/** Tauri injects this global into its webviews before any script runs. */
const TAURI_GLOBAL = "__TAURI_INTERNALS__";

/** True inside the Tauri desktop app. */
export function isDesktop(host: object = globalThis): boolean {
	return TAURI_GLOBAL in host;
}

export function appMode(host?: object): AppMode {
	return isDesktop(host) ? "desktop" : "container";
}

/** The last segment of a path, with either separator. */
export function baseName(path: string): string {
	return path.split(/[/\\]/).filter(Boolean).at(-1) ?? path;
}

/** Container mode: files are chosen in the app's own browser over the files service. */
export function createContainerFilePicker(open: OpenFileBrowser): FilePicker {
	return {
		mode: "container",
		pickFiles: (options = {}) => open({ multiple: true, directory: false, ...options })
	};
}

/**
 * Desktop mode: native dialogs through Tauri's dialog plugin.
 * The implementation comes with the Tauri task (TASK-44); until then every call rejects.
 */
export function createDesktopFilePicker(): FilePicker {
	return {
		mode: "desktop",
		pickFiles: () => Promise.reject(new Error("The native file dialog is not wired up yet (TASK-44)"))
	};
}

/** The picker for the current mode. */
export function createFilePicker(open: OpenFileBrowser, host?: object): FilePicker {
	return isDesktop(host) ? createDesktopFilePicker() : createContainerFilePicker(open);
}
