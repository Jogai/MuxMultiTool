import { createFilePicker, type FilePicker } from "~/core/libs/filePicker";
import { useFilePickerStore } from "~/core/stores/filePicker";

/**
 * The file picker for the current mode. Components call `pickFiles()` on it and never know whether a
 * native dialog or the in-app browser answered.
 */
export function useFilePicker(): FilePicker {
	const store = useFilePickerStore();
	return createFilePicker((options) => store.request(options));
}
