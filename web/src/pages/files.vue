<script setup lang="ts">
import { useFolderBrowser } from "~/core/composables/useFolderBrowser";
import { useStreamReport } from "~/core/composables/useStreamReport";
import type { AppError } from "~/core/libs/errors";
import { entriesForPicked } from "~/core/libs/pickedFiles";
import type { ReportLayout } from "~/core/libs/reportText";
import type { StreamRef } from "~/features/files/components/FilesReport.vue";
import { type Entry, FilesService } from "~/generated/muxmultitool/files/v1/files_pb";

/**
 * The Files screen (sketch 01), the entry point of the app: the folder tree and the file table of the
 * current folder on top, the stream report of the selection below, with the details of one stream
 * beside it. The current folder is in the URL (`?folder=…`), the selection in its store, which every
 * other screen starts from.
 */
const { t } = useI18n();
const route = useRoute();
const router = useRouter();
const client = useRpcClient(FilesService);
const browser = useFolderBrowser(client);
const report = useStreamReport(client);
const selection = useFileSelectionStore();
const picker = useFilePicker();

// The current folder follows the URL
const folderParam = computed(() => (typeof route.query.folder === "string" ? route.query.folder : ""));
watch(folderParam, (id) => void browser.open(id), { immediate: true });
function navigate(id: string) {
	void router.replace({ query: { ...route.query, folder: id || undefined } });
}

// The tree: the roots once, the rest as folders are expanded
const roots = ref<Entry[]>();
const rootsError = ref<AppError>();
onMounted(async () => {
	try {
		roots.value = await browser.subfolders("");
	} catch (cause) {
		rootsError.value = toAppError(cause);
	}
});
const expandIds = computed(() => new Set(browser.breadcrumbs.value.map((entry) => entry.id)));
const folderName = computed(() => browser.folder.value?.name ?? t("files.roots"));

// The report follows the selection: a quick report first, so the streams show at once, then the
// packet pass for exact sizes. Files already reported are not probed again.
watch(
	() => selection.ids,
	async (ids) => {
		await report.load(ids);
		await report.measure(ids);
	},
	{ immediate: true }
);
const reports = computed(() => report.reportsFor(selection.ids));
const pendingIds = computed(() => selection.ids.filter((id) => !report.byId(id)));
const layout = ref<ReportLayout>("grouped");

// Wildcard selection
const finding = ref(false);
const findError = ref<AppError>();
const findMessage = ref<string>();
async function selectMatching(patterns: string[], recursive: boolean) {
	finding.value = true;
	findError.value = undefined;
	findMessage.value = undefined;
	try {
		const found = await browser.find(patterns, recursive);
		const before = selection.count;
		selection.add(found);
		const added = selection.count - before;
		findMessage.value =
			found.length === 0
				? t("files.pattern.none", { patterns: patterns.join(" ") })
				: t("files.pattern.added", { n: added, matched: found.length }, added);
	} catch (cause) {
		findError.value = toAppError(cause);
	} finally {
		finding.value = false;
	}
}

// "Open files…" goes through the shell's picker: the in-app browser here, a native dialog on desktop
const opening = ref(false);
const openError = ref<AppError>();
async function openFiles() {
	opening.value = true;
	openError.value = undefined;
	try {
		const picked = await picker.pickFiles({ multiple: true });
		selection.add(await entriesForPicked(client, picker.mode, picked));
	} catch (cause) {
		openError.value = toAppError(cause);
	} finally {
		opening.value = false;
	}
}

// Details of one stream, beside the report
const selectedStream = ref<StreamRef>();
const selectedReport = computed(() => (selectedStream.value ? report.byId(selectedStream.value.fileId)?.report : undefined));
const selectedSummary = computed(() => selectedReport.value?.streams.find((stream) => stream.index === selectedStream.value?.index));
const selectedName = computed(() => selectedReport.value?.entry?.name ?? selectedStream.value?.fileId ?? "");
// A stream whose file left the selection closes its details
watch(
	() => selection.ids,
	(ids) => {
		if (selectedStream.value && !ids.includes(selectedStream.value.fileId)) {
			selectedStream.value = undefined;
		}
	}
);
</script>

<template>
	<section class="flex min-h-full flex-col">
		<h2 class="sr-only">{{ t("files.title") }}</h2>
		<FilesPathBar
			:breadcrumbs="browser.breadcrumbs.value"
			:busy="finding || opening"
			:can-find="!browser.isRoots.value"
			@navigate="navigate"
			@find="selectMatching"
			@open="openFiles"
		/>
		<div v-if="findError || openError || findMessage" class="flex flex-col gap-2 border-b border-border px-4 py-2 text-sm">
			<ErrorPanel v-if="findError" :error="findError" />
			<ErrorPanel v-if="openError" :error="openError" />
			<p v-if="findMessage" class="text-muted" role="status">{{ findMessage }}</p>
		</div>

		<div class="grid flex-1 grid-cols-[14rem_minmax(0,1fr)]">
			<div class="overflow-x-hidden border-r border-border py-2 pr-1 pl-2">
				<FilesFolderTree
					:roots="roots"
					:current-id="browser.folderId.value"
					:expand-ids="expandIds"
					:load="browser.subfolders"
					:error="rootsError"
					@navigate="navigate"
				/>
			</div>

			<div class="flex min-w-0 flex-col">
				<FilesTable
					:folder-name="folderName"
					:files="browser.files.value"
					:reports="report.cache.value"
					:loading="browser.loading.value"
					:error="browser.error.value"
					class="max-h-[45vh] overflow-auto border-b border-border"
				/>

				<div class="grid min-w-0 gap-4 p-4" :class="{ 'xl:grid-cols-[minmax(0,1fr)_24rem]': selectedSummary }">
					<FilesReport
						v-model:layout="layout"
						:reports="reports"
						:pending-ids="pendingIds"
						:loading="report.loading.value"
						:error="report.error.value"
						:measured-all="report.measuredAll.value"
						:selected="selectedStream"
						class="rounded-lg border border-border"
						@select="selectedStream = $event"
						@measure="report.measure(selection.ids)"
					/>
					<FilesDetails
						v-if="selectedStream && selectedSummary"
						:file-id="selectedStream.fileId"
						:file-name="selectedName"
						:summary="selectedSummary"
						:fetch="report.details"
						@close="selectedStream = undefined"
					/>
				</div>
			</div>
		</div>

		<FilesActionBar :count="selection.count" @clear="selection.clear()" />
	</section>
</template>
