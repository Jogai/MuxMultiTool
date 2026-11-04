import { create } from "@bufbuild/protobuf";

import {
	type CheckMergeResponse,
	type MergeMismatch,
	type MergeOperation,
	MergeOperationSchema
} from "~/generated/muxmultitool/merge/v1/merge_pb";

/** What the merge form holds: the parts come from the file selection, in order. */
export interface MergeForm {
	inputs: readonly string[];
	/** Streams of the first file to leave out. */
	dropStreams: readonly number[];
	chapters: boolean;
	/** Empty names `<first file>-merged.<extension>` next to the first file. */
	output: string;
}

/** The operation the check and the submit both take. */
export function toOperation(form: MergeForm): MergeOperation {
	return create(MergeOperationSchema, {
		inputs: [...form.inputs],
		output: form.output.trim(),
		dropStreams: [...form.dropStreams],
		chapters: form.chapters
	});
}

/** `dropped` with the stream added or removed. */
export function toggleDrop(dropped: readonly number[], index: number): number[] {
	return dropped.includes(index) ? dropped.filter((value) => value !== index) : [...dropped, index].toSorted((a, b) => a - b);
}

/** The mismatches of the stream's cell for the part at `part` (the cells are in part order). */
export function mismatchesOfPart(mismatches: readonly MergeMismatch[], part: number): MergeMismatch[] {
	return mismatches.filter((mismatch) => mismatch.part === part);
}

/** Whether the merge may start: the check passed for exactly these inputs. */
export function canStart(check: CheckMergeResponse | undefined, inputs: number): boolean {
	return check !== undefined && check.canMerge && check.parts.length === inputs && inputs >= 2;
}
