<script setup lang="ts">
import { hasExtension, useFolderBrowser } from "~/core/composables/useFolderBrowser";
import { pickedFromEntry } from "~/core/libs/pickedFiles";
import type { Entry } from "~/generated/muxmultitool/files/v1/files_pb";

/**
 * The container file browser, opened by `pickFiles()` through the file-picker store: the folders of
 * the media roots over the files service, with the files of the accepted types. It answers with the
 * entries' opaque ids as paths (see `pickedFromEntry`). Reopens in the folder it was last in.
 */
const { t } = useI18n();
const picker = useFilePickerStore();
const browser = useFolderBrowser();

const options = computed(() => picker.pending?.options);
const multiple = computed(() => options.value?.multiple !== false);
const directory = computed(() => options.value?.directory === true);
const title = computed(() =>
	directory.value ? t("filePicker.titleDirectory") : multiple.value ? t("filePicker.title") : t("filePicker.titleSingle")
);

const chosen = ref(new Map<string, Entry>());
const files = computed(() => browser.files.value.filter((entry) => hasExtension(entry.name, options.value?.extensions)));

watch(
	() => picker.isOpen,
	(open) => {
		if (open) {
			chosen.value = new Map();
			void browser.open(browser.folderId.value);
		}
	},
	{ immediate: true }
);

function toggle(entry: Entry) {
	const next = multiple.value ? new Map(chosen.value) : new Map<string, Entry>();
	if (!next.delete(entry.id)) {
		next.set(entry.id, entry);
	}
	chosen.value = next;
}

function choose() {
	picker.finish([...chosen.value.values()].map(pickedFromEntry));
}

function chooseFolder() {
	const folder = browser.folder.value;
	if (folder) {
		picker.finish([pickedFromEntry(folder)]);
	}
}
</script>

<template>
	<RekaDialogRoot :open="picker.isOpen" @update:open="(value) => !value && picker.cancel()">
		<RekaDialogPortal>
			<RekaDialogOverlay class="fixed inset-0 bg-overlay" />
			<RekaDialogContent
				class="fixed top-1/2 left-1/2 flex max-h-[min(40rem,calc(100vh-2rem))] w-[min(44rem,calc(100vw-2rem))] -translate-1/2 flex-col rounded-lg border border-border bg-surface p-5 shadow-lg"
			>
				<RekaDialogTitle class="text-lg font-bold">{{ title }}</RekaDialogTitle>
				<RekaDialogDescription class="mt-1 text-sm text-muted">
					{{ directory ? t("filePicker.descriptionDirectory") : t("filePicker.description") }}
				</RekaDialogDescription>

				<nav :aria-label="t('files.breadcrumb')" class="mt-3 flex items-center gap-0.5 font-mono text-sm">
					<button
						type="button"
						class="flex size-7 shrink-0 items-center justify-center rounded-md text-muted hover:bg-surface-sunken hover:text-text focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-40"
						:aria-label="t('filePicker.up')"
						:disabled="browser.parentId.value === undefined"
						@click="browser.up()"
					>
						<i-ph-arrow-up class="size-4" />
					</button>
					<button
						type="button"
						class="rounded px-1 py-0.5 hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent"
						:class="browser.isRoots.value ? 'text-text' : 'text-muted'"
						@click="browser.open()"
					>
						{{ t("filePicker.roots") }}
					</button>
					<template v-for="(crumb, index) in browser.breadcrumbs.value" :key="crumb.id">
						<i-ph-caret-right class="size-3.5 shrink-0 text-muted" aria-hidden="true" />
						<button
							type="button"
							class="truncate rounded px-1 py-0.5 hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent"
							:class="index === browser.breadcrumbs.value.length - 1 ? 'text-text' : 'text-muted'"
							@click="browser.open(crumb.id)"
						>
							{{ crumb.name }}
						</button>
					</template>
				</nav>

				<div class="mt-2 min-h-48 flex-1 overflow-auto rounded-md border border-border" :aria-busy="browser.loading.value">
					<ErrorPanel v-if="browser.error.value" :error="browser.error.value" class="m-2" />
					<ul v-else-if="browser.folders.value.length > 0 || files.length > 0" class="py-1 text-sm">
						<li v-for="folder in browser.folders.value" :key="folder.id">
							<button
								type="button"
								class="flex w-full items-center gap-2 px-3 py-1 text-left hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent"
								@click="browser.open(folder.id)"
							>
								<i-ph-folder class="size-4 shrink-0 text-muted" aria-hidden="true" />
								<span class="truncate">{{ folder.name }}</span>
								<i-ph-caret-right class="ml-auto size-3.5 shrink-0 text-muted" aria-hidden="true" />
							</button>
						</li>
						<li v-for="file in files" :key="file.id">
							<label
								class="flex w-full cursor-pointer items-center gap-2 px-3 py-1 hover:bg-surface-sunken"
								:class="{ 'bg-accent-surface': chosen.has(file.id), 'opacity-50': directory }"
							>
								<input
									:type="multiple ? 'checkbox' : 'radio'"
									class="accent-accent"
									:checked="chosen.has(file.id)"
									:disabled="directory"
									@change="toggle(file)"
								/>
								<i-ph-file-video class="size-4 shrink-0 text-muted" aria-hidden="true" />
								<span class="truncate">{{ file.name }}</span>
								<span class="ml-auto shrink-0 font-mono text-xs text-muted">{{ formatBytes(file.sizeBytes) }}</span>
							</label>
						</li>
					</ul>
					<p v-else-if="!browser.loading.value" class="p-3 text-sm text-muted">
						{{
							browser.files.value.length > 0 && options?.extensions?.length
								? t("filePicker.noMatches", { extensions: options.extensions.join(", ") })
								: t("filePicker.empty")
						}}
					</p>
				</div>

				<div class="mt-4 flex items-center gap-2">
					<span v-if="!directory && chosen.size > 0" class="text-sm text-muted">{{ t("filePicker.chosen", { n: chosen.size }) }}</span>
					<RekaDialogClose
						class="ml-auto rounded-md border border-border px-3 py-1.5 text-sm hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent"
					>
						{{ t("filePicker.cancel") }}
					</RekaDialogClose>
					<button
						v-if="directory"
						type="button"
						class="rounded-md bg-accent px-3 py-1.5 text-sm text-on-accent focus-visible:outline-2 focus-visible:outline-accent disabled:cursor-not-allowed disabled:opacity-50"
						:disabled="!browser.folder.value"
						@click="chooseFolder"
					>
						{{ t("filePicker.chooseFolder") }}
					</button>
					<button
						v-else
						type="button"
						class="rounded-md bg-accent px-3 py-1.5 text-sm text-on-accent focus-visible:outline-2 focus-visible:outline-accent disabled:cursor-not-allowed disabled:opacity-50"
						:disabled="chosen.size === 0"
						@click="choose"
					>
						{{ t("filePicker.choose") }}
					</button>
				</div>
			</RekaDialogContent>
		</RekaDialogPortal>
	</RekaDialogRoot>
</template>
