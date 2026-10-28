import { defineStore } from "pinia";
import { computed, ref } from "vue";

import type { PickFilesOptions, PickedFile } from "~/core/libs/filePicker";

interface PendingPick {
	options: PickFilesOptions;
	resolve: (files: PickedFile[]) => void;
}

/**
 * The open request of the container file browser dialog.
 * `pickFiles()` puts a request here, `<FilePickerDialog>` in the app shell shows it and finishes it.
 */
export const useFilePickerStore = defineStore("filePicker", () => {
	const pending = ref<PendingPick>();
	const isOpen = computed(() => pending.value !== undefined);

	function request(options: PickFilesOptions): Promise<PickedFile[]> {
		// A second request replaces the first, which then counts as cancelled
		pending.value?.resolve([]);
		return new Promise((resolve) => {
			pending.value = { options, resolve };
		});
	}

	function finish(files: PickedFile[]): void {
		pending.value?.resolve(files);
		pending.value = undefined;
	}

	function cancel(): void {
		finish([]);
	}

	return { pending, isOpen, request, finish, cancel };
});
