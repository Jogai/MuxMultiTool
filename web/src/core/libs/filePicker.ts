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

export interface PickSavePathOptions {
	/** The suggested file name, e.g. `movie.cut.mkv`. */
	defaultName: string;
	/** Folder to start in: a path under a media root in the container, any path on desktop. */
	startIn?: string;
	/** Accepted extensions without the dot. Empty or absent means all files. */
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
	/**
	 * Where to write a new file: the native save dialog on desktop; in the container, a folder from the in-app
	 * browser with the suggested name appended. Resolves with nothing when the user cancelled.
	 */
	pickSavePath(options: PickSavePathOptions): Promise<PickedFile | undefined>;
}

/** What the container picker delegates to: the in-app file browser dialog. */
export type OpenFileBrowser = (options: PickFilesOptions) => Promise<PickedFile[]>;

/** Tauri injects this global into its webviews before any script runs. */
const TAURI_GLOBAL = "__TAURI_INTERNALS__";

/** The part of Tauri's injected global that the picker uses: the IPC call. */
interface TauriInternals {
	invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>;
}

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

/** A path with `name` appended, using the separator the folder path already uses. */
export function joinPath(folder: string, name: string): string {
	const separator = folder.includes("\\") && !folder.includes("/") ? "\\" : "/";
	const trimmed = folder.endsWith(separator) ? folder.slice(0, -1) : folder;
	return `${trimmed}${separator}${name}`;
}

function toPickedFile(path: string): PickedFile {
	return { path, name: baseName(path) };
}

/** Container mode: files are chosen in the app's own browser over the files service. */
export function createContainerFilePicker(open: OpenFileBrowser): FilePicker {
	return {
		mode: "container",
		pickFiles: (options = {}) => open({ multiple: true, directory: false, ...options }),
		async pickSavePath({ defaultName, startIn }) {
			const [folder] = await open({ multiple: false, directory: true, startIn });
			return folder && toPickedFile(joinPath(folder.path, defaultName));
		}
	};
}

/** The dialog plugin's filter: one entry named after the extensions it accepts. */
function filters(extensions: string[] | undefined): { name: string; extensions: string[] }[] {
	if (!extensions?.length) return [];
	return [{ name: extensions.map((extension) => `*.${extension}`).join(", "), extensions }];
}

/**
 * The dialog plugin answers with the chosen path(s) as plain strings, or null when the user cancelled:
 * `string[]` for files and folders with `multiple`, a single `string` otherwise.
 */
function toPickedFiles(response: unknown): PickedFile[] {
	if (typeof response === "string") return [toPickedFile(response)];
	if (Array.isArray(response)) return response.filter((path) => typeof path === "string").map(toPickedFile);
	return [];
}

/**
 * Desktop mode: native dialogs through Tauri's dialog plugin, called over the IPC that the app's capability
 * file allows for the embedded server's origin. The chosen paths are absolute paths on the user's machine,
 * which the backend accepts in desktop mode (`FilesService/OpenPaths`).
 */
export function createDesktopFilePicker(host: object = globalThis): FilePicker {
	const { invoke } = (host as Record<string, unknown>)[TAURI_GLOBAL] as TauriInternals;
	return {
		mode: "desktop",
		async pickFiles({ multiple = true, directory = false, startIn, extensions } = {}) {
			const response = await invoke("plugin:dialog|open", {
				options: { multiple, directory, defaultPath: startIn, filters: filters(extensions) }
			});
			return toPickedFiles(response);
		},
		async pickSavePath({ defaultName, startIn, extensions }) {
			const defaultPath = startIn ? joinPath(startIn, defaultName) : defaultName;
			const response = await invoke("plugin:dialog|save", {
				options: { defaultPath, filters: filters(extensions) }
			});
			return typeof response === "string" ? toPickedFile(response) : undefined;
		}
	};
}

/** The picker for the current mode. */
export function createFilePicker(open: OpenFileBrowser, host?: object): FilePicker {
	return isDesktop(host) ? createDesktopFilePicker(host) : createContainerFilePicker(open);
}
