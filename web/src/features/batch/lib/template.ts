import { extensionOf, stem } from "~/core/libs/fileNames";

/**
 * The output name template of a batch, as `core::batch::template` renders it on the server, so the
 * Batch screen can show the names a template gives before the dry run and flag a template the server
 * would refuse. The rendered name is a bare file name for the output folder, and one every platform
 * can write: a separator in the template itself is an error; inside a field's value a path separator,
 * one of `<>:"|?*` or a control character becomes `_`; trailing dots and spaces are dropped from the
 * name, and a name Windows reserves for a device (`CON`, `NUL.txt`) gets a `_` in front. Keep the
 * rules the same as the server's: it is the one that names the real outputs.
 */

/** The placeholders a template may use, without braces. */
export const PLACEHOLDERS = ["name", "ext", "lang", "date", "n", "parent"] as const;

export type Placeholder = (typeof PLACEHOLDERS)[number];

/** The values of one file, one per placeholder. */
export type TemplateFields = Record<Placeholder, string>;

/** What is wrong with a template, as the server reports it. */
export type TemplateProblem =
	| { kind: "empty" }
	| { kind: "separator" }
	| { kind: "unclosed"; position: number }
	| { kind: "unknown"; placeholder: string };

function isPlaceholder(name: string): name is Placeholder {
	return (PLACEHOLDERS as readonly string[]).includes(name);
}

/** One piece of a parsed template: literal text, or a field to fill in. */
export type TemplatePart = { text: string } | { field: Placeholder };

/** Splits the template into text and fields; `undefined` when it has a problem (see `checkTemplate`). */
export function parseTemplate(template: string): TemplatePart[] | undefined {
	return checkTemplate(template) === undefined ? parts(template) : undefined;
}

/** The problem of a template, or `undefined` when the server would accept it. */
export function checkTemplate(template: string): TemplateProblem | undefined {
	if (template === "") {
		return { kind: "empty" };
	}
	if (template.includes("/") || template.includes("\\")) {
		return { kind: "separator" };
	}
	let rest = template;
	let position = 0;
	for (let open = rest.indexOf("{"); open !== -1; open = rest.indexOf("{")) {
		const close = rest.indexOf("}", open);
		if (close === -1) {
			return { kind: "unclosed", position: position + open };
		}
		const name = rest.slice(open + 1, close);
		if (!isPlaceholder(name)) {
			return { kind: "unknown", placeholder: name };
		}
		position += close + 1;
		rest = rest.slice(close + 1);
	}
	return undefined;
}

function parts(template: string): TemplatePart[] {
	const result: TemplatePart[] = [];
	let rest = template;
	for (let open = rest.indexOf("{"); open !== -1; open = rest.indexOf("{")) {
		const close = rest.indexOf("}", open);
		if (open > 0) {
			result.push({ text: rest.slice(0, open) });
		}
		const name = rest.slice(open + 1, close);
		if (isPlaceholder(name)) {
			result.push({ field: name });
		}
		rest = rest.slice(close + 1);
	}
	if (rest !== "") {
		result.push({ text: rest });
	}
	return result;
}

/** The placeholders the template uses, in order of first use. */
export function placeholdersOf(template: string): Placeholder[] {
	const used: Placeholder[] = [];
	for (const part of parts(template)) {
		if ("field" in part && !used.includes(part.field)) {
			used.push(part.field);
		}
	}
	return used;
}

/** A field's value as a file name may hold it: separators, `<>:"|?*` and control characters become `_`. */
export function safeValue(value: string): string {
	return value.replaceAll(/[/\\<>:"|?*\p{Cc}]/gu, "_");
}

/** The device names Windows reserves, in any case: `CON`, `PRN`, `AUX`, `NUL`, `COM0`-`COM9`, `LPT0`-`LPT9`. */
const RESERVED_NAME = /^(?:CON|PRN|AUX|NUL|COM\d|LPT\d)$/iu;

/**
 * The rendered name as one Windows can write: without trailing dots and spaces (Windows drops them
 * silently), and with a `_` in front when the part before the first dot, less trailing spaces, is a
 * device name.
 */
export function safeName(name: string): string {
	const trimmed = name.replace(/[. ]+$/u, "");
	const stem = (trimmed.split(".", 1)[0] ?? "").replace(/ +$/u, "");
	return RESERVED_NAME.test(stem) ? `_${trimmed}` : trimmed;
}

/**
 * Fills the template in for one file: one file name every platform can write. A template with a
 * problem renders as written, so the preview never hides what the user typed.
 */
export function renderTemplate(template: string, fields: TemplateFields): string {
	const parsed = parseTemplate(template);
	if (!parsed) {
		return template;
	}
	return safeName(parsed.map((part) => ("text" in part ? part.text : safeValue(fields[part.field]))).join(""));
}

/** The file's number in the batch, 1-based and zero-padded to the count's width. */
export function padNumber(index: number, count: number): string {
	return String(index + 1).padStart(String(Math.max(count, 1)).length, "0");
}

/** A moment as `YYYY-MM-DD` in UTC, the `{date}` of a batch. */
export function utcDate(now: Date): string {
	return now.toISOString().slice(0, 10);
}

/**
 * The name of the folder a file is in, from a path with `/` separators: the entry's path relative to
 * its media root, or an absolute path. `fallback` (the searched folder's name) when the path has no
 * folder part.
 */
export function parentName(path: string, fallback = ""): string {
	const segments = path.split(/[/\\]/).filter(Boolean);
	return segments.length > 1 ? (segments.at(-2) ?? fallback) : fallback;
}

/** The languages of the kept audio streams as `{lang}` spells them: `eng-fre`, or `und` when none is tagged. */
export function languagesOf(streams: readonly { kept: boolean; isAudio: boolean; language: string }[]): string {
	const languages: string[] = [];
	for (const stream of streams) {
		const language = stream.language.trim().toLowerCase();
		if (stream.kept && stream.isAudio && language !== "" && language !== "und" && !languages.includes(language)) {
			languages.push(language);
		}
	}
	return languages.length > 0 ? languages.join("-") : "und";
}

/** A file as the preview needs it: its name and its path for `{parent}`. */
export interface PreviewFile {
	name: string;
	path: string;
}

export interface PreviewOptions {
	/** The `{date}`; today when absent. */
	date?: string;
	/** The name of the searched folder, the `{parent}` of a file at its top. */
	folderName?: string;
	/** The `{lang}` before the files are probed: the dry run knows the real one. */
	lang?: string;
}

/** The fields of one file of a batch, as the live preview knows them before the dry run. */
export function previewFields(file: PreviewFile, index: number, count: number, options: PreviewOptions = {}): TemplateFields {
	return {
		name: stem(file.name),
		ext: extensionOf(file.name),
		lang: options.lang ?? "und",
		date: options.date ?? utcDate(new Date()),
		n: padNumber(index, count),
		parent: parentName(file.path, options.folderName)
	};
}

/** One line of the live preview: the input's name and the output's. */
export interface PreviewName {
	input: string;
	output: string;
}

/** The output names of the first `limit` files, in their order. */
export function previewNames(template: string, files: readonly PreviewFile[], options: PreviewOptions = {}, limit = 5): PreviewName[] {
	return files.slice(0, limit).map((file, index) => ({
		input: file.name,
		output: renderTemplate(template, previewFields(file, index, files.length, options))
	}));
}

/** The containers the server picks by the output's extension, in `Container::from_extension` order. */
const CONTAINER_EXTENSIONS: readonly { extensions: readonly string[]; name: string }[] = [
	{ extensions: ["mkv", "mka", "mks"], name: "MKV" },
	{ extensions: ["mp4", "m4v", "m4a"], name: "MP4" },
	{ extensions: ["mov"], name: "MOV" },
	{ extensions: ["webm"], name: "WebM" }
];

/** The container an output name's extension picks; `undefined` for an extension the server does not know. */
export function containerOf(fileName: string): string | undefined {
	const extension = extensionOf(fileName).toLowerCase();
	return CONTAINER_EXTENSIONS.find((container) => container.extensions.includes(extension))?.name;
}

/**
 * What the template's own extension says about the target: a container's name when the template
 * ends in one of its extensions, `"input"` when it ends in `{ext}` (every file keeps its own), and
 * `undefined` when the extension is unknown or missing, which the dry run reports per file.
 */
export function templateTarget(template: string): string | undefined {
	if (template.endsWith(".{ext}")) {
		return "input";
	}
	const parsed = parseTemplate(template);
	const last = parsed?.at(-1);
	if (!last || !("text" in last)) {
		return undefined;
	}
	const extension = /\.([^.]+)$/.exec(last.text)?.[1];
	return extension === undefined ? undefined : containerOf(`x.${extension}`);
}
