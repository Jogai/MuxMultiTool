import { type AppError, toAppError } from "~/core/libs/errors";
import { entriesForPicked } from "~/core/libs/pickedFiles";
import { useRpcClient } from "~/core/libs/rpc";
import { FilesService } from "~/generated/muxmultitool/files/v1/files_pb";

/**
 * The one file a screen works on (Images, Crop): the file named by the `file` query (`?file=<id>`) or,
 * without one, the first file of the selection, its display name, and an Open button that picks
 * another file and puts it in the query.
 */
export function useOpenedFile(extensions: readonly string[]) {
	const route = useRoute();
	const router = useRouter();
	const picker = useFilePicker();
	const selection = useFileSelectionStore();
	const filesClient = useRpcClient(FilesService);

	const fileId = computed(() => (typeof route.query.file === "string" && route.query.file !== "" ? route.query.file : selection.ids[0]));
	const entryName = ref<string>();
	watch(
		fileId,
		async (id) => {
			entryName.value = undefined;
			if (!id) {
				return;
			}
			const known = selection.files.find((file) => file.id === id);
			if (known) {
				entryName.value = known.name;
				return;
			}
			try {
				const response = await filesClient.getEntries({ ids: [id] });
				entryName.value = response.entries[0]?.name || undefined;
			} catch {
				// The player's report shows the error; the name falls back to the id's last segment
			}
		},
		{ immediate: true }
	);
	const fileName = computed(() => entryName.value ?? baseName(fileId.value ?? ""));

	const error = ref<AppError>();
	async function openFile() {
		error.value = undefined;
		try {
			const picked = await picker.pickFiles({ multiple: false, extensions: [...extensions] });
			const [entry] = await entriesForPicked(filesClient, picker.mode, picked);
			const id = entry?.id ?? picked[0]?.path;
			if (id) {
				await router.replace({ query: { ...route.query, file: id } });
			}
		} catch (cause) {
			error.value = toAppError(cause);
		}
	}

	return { fileId, fileName, error, openFile };
}
