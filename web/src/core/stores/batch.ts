import { defineStore } from "pinia";
import { ref, shallowRef } from "vue";

import { type BatchForm, type BatchSubmission, emptyBatchForm } from "~/features/batch/lib/batch";

/**
 * The Batch screen's form and the batch it last queued, kept across navigation: a user who goes to the
 * Jobs screen while the batch runs finds its per-file progress again on coming back, and the Presets
 * screen's "Apply to a batch" puts the preset here before it opens the screen.
 */
export const useBatchStore = defineStore("batch", () => {
	const form = ref<BatchForm>(emptyBatchForm());
	const submission = shallowRef<BatchSubmission>();

	/** Makes the preset the operation of the next batch; the folders and patterns stay as they are. */
	function applyPreset(presetId: string): void {
		form.value = { ...form.value, source: "preset", presetId };
		submission.value = undefined;
	}

	function reset(): void {
		form.value = emptyBatchForm();
		submission.value = undefined;
	}

	return { form, submission, applyPreset, reset };
});
