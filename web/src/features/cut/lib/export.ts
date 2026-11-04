import { create } from "@bufbuild/protobuf";

import { extensionOf, stem } from "~/core/libs/fileNames";
import { CutExport, type CutOperation, CutOperationSchema } from "~/generated/muxmultitool/cut/v1/cut_pb";

import type { EditorState } from "./segments";

/** The operation `PlanCut` and `SubmitJob` both take. */
export function toCutOperation(input: string, state: EditorState, output: string): CutOperation {
	return create(CutOperationSchema, {
		input,
		segments: state.segments.map((segment) => ({ startSeconds: segment.start, endSeconds: segment.end })),
		segmentMode: state.segmentMode,
		cutMode: state.cutMode,
		export: state.export,
		output: output.trim()
	});
}

/** Whether there is something to plan or export. */
export function canExport(state: EditorState, output: string): boolean {
	return state.segments.length > 0 && output.trim() !== "";
}

/** The last path segment without its extension, and the extension with its dot (empty when none). */
export function splitName(path: string): { folder: string; stem: string; extension: string } {
	const slash = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
	const folder = slash >= 0 ? path.slice(0, slash + 1) : "";
	const name = path.slice(slash + 1);
	const extension = extensionOf(name);
	return { folder, stem: stem(name), extension: extension === "" ? "" : `.${extension}` };
}

/**
 * The output to suggest for a source: `<name>-cut.<ext>` next to it for a merged export, the template
 * `<name>-cut-{n}.<ext>` for separate files (`{n}` is the part number, see `CutOperation.output`).
 */
export function defaultOutput(sourcePath: string, mode: CutExport): string {
	if (sourcePath === "") {
		return "";
	}
	const { folder, stem, extension } = splitName(sourcePath);
	return mode === CutExport.MERGED ? `${folder}${stem}-cut${extension}` : `${folder}${stem}-cut-{n}${extension}`;
}

/**
 * Whether `output` is still the suggestion for `sourcePath` in one export mode, so switching the mode
 * can replace it with the other mode's suggestion without overwriting what the user typed.
 */
export function isDefaultOutput(output: string, sourcePath: string): boolean {
	return output === defaultOutput(sourcePath, CutExport.MERGED) || output === defaultOutput(sourcePath, CutExport.SEPARATE_FILES);
}
