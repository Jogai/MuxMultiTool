<script setup lang="ts">
import type { CachedReport } from "~/core/composables/useStreamReport";
import type { AppError } from "~/core/libs/errors";
import { containerLabel, formatDuration, formatGb, videoLabel } from "~/core/libs/reportFormat";
import type { Entry, FileReport } from "~/generated/muxmultitool/files/v1/files_pb";

/**
 * The media files of the current folder with a checkbox per row: the selection. The container,
 * duration, stream count and video columns fill in from the stream report once a file is selected,
 * because listing a folder only knows names and sizes.
 */
const {
	folderName,
	files,
	reports,
	loading = false,
	error
} = defineProps<{
	folderName: string;
	files: Entry[];
	reports: ReadonlyMap<string, CachedReport>;
	loading?: boolean;
	error?: AppError;
}>();
const { t } = useI18n();
const selection = useFileSelectionStore();

const selectedHere = computed(() => files.filter((file) => selection.has(file.id)).length);
const allSelected = computed(() => files.length > 0 && selectedHere.value === files.length);
const someSelected = computed(() => selectedHere.value > 0 && !allSelected.value);

function toggleAll() {
	if (allSelected.value) {
		selection.remove(files.map((file) => file.id));
	} else {
		selection.add(files);
	}
}

const reportOf = (file: Entry): FileReport | undefined => reports.get(file.id)?.report;
const containerOf = (file: Entry) => {
	const container = reportOf(file)?.container;
	return container ? containerLabel(container, file.name) : undefined;
};
const durationOf = (file: Entry) => {
	const container = reportOf(file)?.container;
	return container ? formatDuration(container.durationSeconds) : undefined;
};
const streamsOf = (file: Entry) => {
	const report = reportOf(file);
	return report && !report.error ? report.streams.length : undefined;
};
const videoOf = (file: Entry) => {
	const report = reportOf(file);
	return report ? videoLabel(report.streams) : undefined;
};
</script>

<template>
	<section class="min-w-0">
		<ErrorPanel v-if="error" :error="error" class="m-4" />
		<p v-else-if="loading && files.length === 0" class="px-4 py-3 text-sm text-muted">{{ t("files.table.loading") }}</p>
		<p v-else-if="files.length === 0" class="px-4 py-3 text-sm text-muted">{{ t("files.table.empty") }}</p>
		<table v-else class="w-full border-collapse text-sm" :aria-busy="loading">
			<caption class="px-4 py-2 text-left text-sm text-muted">
				{{ t("files.table.caption", { folder: folderName, files: files.length, selected: selectedHere }, files.length) }}
			</caption>
			<thead class="text-left text-xs text-muted uppercase">
				<tr class="border-b border-border">
					<th scope="col" class="w-8 py-2 pr-2 pl-4">
						<input
							type="checkbox"
							class="accent-accent"
							:checked="allSelected"
							:indeterminate="someSelected"
							:aria-label="t('files.table.selectAll')"
							@change="toggleAll"
						/>
					</th>
					<th scope="col" class="w-full py-2 pr-3 font-normal">{{ t("files.table.name") }}</th>
					<th scope="col" class="py-2 pr-3 font-normal">{{ t("files.table.container") }}</th>
					<th scope="col" class="py-2 pr-3 text-right font-normal">{{ t("files.table.duration") }}</th>
					<th scope="col" class="py-2 pr-3 text-right font-normal">{{ t("files.table.streams") }}</th>
					<th scope="col" class="py-2 pr-3 text-right font-normal">{{ t("files.table.size") }}</th>
					<th scope="col" class="py-2 pr-4 font-normal">{{ t("files.table.video") }}</th>
				</tr>
			</thead>
			<tbody>
				<tr
					v-for="file in files"
					:key="file.id"
					class="cursor-pointer border-b border-border"
					:class="selection.has(file.id) ? 'bg-accent-surface' : 'hover:bg-surface-raised'"
					:aria-selected="selection.has(file.id)"
					@click="selection.toggle(file)"
				>
					<td class="py-1.5 pr-2 pl-4 align-middle">
						<input
							type="checkbox"
							class="accent-accent"
							:checked="selection.has(file.id)"
							:aria-label="t('files.table.select', { name: file.name })"
							@click.stop
							@change="selection.toggle(file)"
						/>
					</td>
					<td class="max-w-0 truncate py-1.5 pr-3 align-middle" :title="file.path || file.name">{{ file.name }}</td>
					<td class="py-1.5 pr-3 align-middle whitespace-nowrap" :class="{ 'text-muted': !containerOf(file) }">
						{{ containerOf(file) ?? "—" }}
					</td>
					<td class="py-1.5 pr-3 text-right align-middle font-mono whitespace-nowrap" :class="{ 'text-muted': !durationOf(file) }">
						{{ durationOf(file) ?? "—" }}
					</td>
					<td class="py-1.5 pr-3 text-right align-middle font-mono" :class="{ 'text-muted': streamsOf(file) === undefined }">
						{{ streamsOf(file) ?? "—" }}
					</td>
					<td class="py-1.5 pr-3 text-right align-middle font-mono whitespace-nowrap">{{ formatGb(file.sizeBytes) }} GB</td>
					<td class="py-1.5 pr-4 align-middle whitespace-nowrap" :class="{ 'text-muted': !videoOf(file) }">{{ videoOf(file) ?? "—" }}</td>
				</tr>
			</tbody>
		</table>
	</section>
</template>
