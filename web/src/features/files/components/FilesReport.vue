<script setup lang="ts">
import { toggled } from "~/core/libs/collections";
import type { AppError } from "~/core/libs/errors";
import { formatGb, formatStreamSize, streamTypeName } from "~/core/libs/reportFormat";
import { type ReportLayout, containerLine, reportAsText, reportName } from "~/core/libs/reportText";
import { type FileReport, SizeMethod, type StreamSummary } from "~/generated/muxmultitool/files/v1/files_pb";

/** A stream of the report, named by its file and index. */
export interface StreamRef {
	fileId: string;
	index: number;
}

/**
 * The stream report of the selection, as in the sketch: grouped under a heading per file or flat with
 * the file in every row, "Copy as text" for the old script's output, and a row per failed file with
 * its error, so one unreadable file never hides the others. Clicking a stream opens its details.
 */
const {
	reports,
	pendingIds,
	loading = false,
	error,
	measuredAll = false,
	selected
} = defineProps<{
	reports: FileReport[];
	/** Selected files whose report has not arrived yet. */
	pendingIds: string[];
	loading?: boolean;
	error?: AppError;
	measuredAll?: boolean;
	selected?: StreamRef;
}>();
const emit = defineEmits<{ select: [stream: StreamRef]; measure: [] }>();
const layout = defineModel<ReportLayout>("layout", { default: "grouped" });
const { t } = useI18n();

const count = computed(() => reports.length + pendingIds.length);
const collapsed = ref(new Set<string>());
const toggleCollapsed = (fileId: string) => (collapsed.value = toggled(collapsed.value, fileId));

const outputOpen = ref(new Set<string>());
const toggleOutput = (fileId: string) => (outputOpen.value = toggled(outputOpen.value, fileId));

const isSelected = (fileId: string, stream: StreamSummary) => selected?.fileId === fileId && selected.index === stream.index;

/** The rows of the flat layout: every stream of every readable file. */
const flatRows = computed(() => reports.flatMap((report) => (report.error ? [] : report.streams.map((stream) => ({ report, stream })))));
const failed = computed(() => reports.filter((report) => report.error));

const sizeTitle = (stream: StreamSummary) => t(`files.report.sizeMethod.${SizeMethod[stream.sizeMethod].toLowerCase()}`);

const text = computed(() => reportAsText(reports, layout.value));
const clipboard = useClipboard();

const cell = "py-1 pr-3 align-middle";
const rowClass = (fileId: string, stream: StreamSummary) => (isSelected(fileId, stream) ? "bg-accent-surface" : "hover:bg-surface-raised");
</script>

<template>
	<section class="flex min-w-0 flex-col" :aria-busy="loading">
		<header class="flex flex-wrap items-center gap-x-4 gap-y-2 px-4 py-2">
			<h3 class="font-bold">{{ t("files.report.title", { n: count }, count) }}</h3>
			<span v-if="loading" class="text-sm text-muted">{{ t("files.report.loading") }}</span>
			<div class="ml-auto flex flex-wrap items-center gap-2">
				<RekaToggleGroupRoot
					type="single"
					:model-value="layout"
					class="inline-flex rounded-md border border-border p-0.5 text-sm"
					@update:model-value="(value) => (value === 'grouped' || value === 'flat') && (layout = value)"
				>
					<RekaToggleGroupItem
						v-for="option in ['grouped', 'flat'] as const"
						:key="option"
						:value="option"
						class="rounded px-3 py-0.5 text-muted hover:text-text focus-visible:outline-2 focus-visible:outline-accent data-[state=on]:bg-accent data-[state=on]:text-on-accent"
					>
						{{ t(`files.report.${option}`) }}
					</RekaToggleGroupItem>
				</RekaToggleGroupRoot>
				<ShellTooltip :text="measuredAll && reports.length > 0 ? t('files.report.measured') : t('files.report.measureHint')">
					<button
						type="button"
						class="rounded-md border border-border px-3 py-1 text-sm hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent aria-disabled:cursor-not-allowed aria-disabled:opacity-50 aria-disabled:hover:bg-transparent"
						:aria-disabled="loading || reports.length === 0 || measuredAll || undefined"
						@click="!(loading || reports.length === 0 || measuredAll) && emit('measure')"
					>
						{{ t("files.report.measure") }}
					</button>
				</ShellTooltip>
				<button
					type="button"
					class="rounded-md border border-border px-3 py-1 text-sm hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent disabled:cursor-not-allowed disabled:opacity-50"
					:disabled="reports.length === 0"
					@click="clipboard.copy(text)"
				>
					{{ clipboard.copied.value ? t("files.report.copied") : t("files.report.copy") }}
				</button>
			</div>
		</header>

		<ErrorPanel v-if="error" :error="error" class="mx-4 mb-3" />

		<p v-if="count === 0" class="px-4 pb-4 text-sm text-muted">{{ t("files.report.empty") }}</p>

		<table v-else class="w-full border-collapse text-sm">
			<thead class="text-left text-xs text-muted uppercase">
				<tr class="border-b border-border">
					<th scope="col" class="w-full py-2 pr-3 pl-4 font-normal">{{ t("files.report.columns.file") }}</th>
					<th scope="col" class="py-2 pr-3 font-normal">{{ t("files.report.columns.type") }}</th>
					<th scope="col" class="py-2 pr-3 font-normal">{{ t("files.report.columns.codec") }}</th>
					<th scope="col" class="py-2 pr-3 font-normal">{{ t("files.report.columns.language") }}</th>
					<th scope="col" class="py-2 pr-3 font-normal">{{ t("files.report.columns.title") }}</th>
					<th scope="col" class="py-2 pr-3 text-right font-normal whitespace-nowrap">{{ t("files.report.columns.size") }}</th>
					<th scope="col" class="py-2 pr-4 font-normal">{{ t("files.report.columns.flags") }}</th>
				</tr>
			</thead>

			<!-- Grouped: a heading row per file, then its streams -->
			<template v-if="layout === 'grouped'">
				<tbody v-for="report in reports" :key="report.fileId" class="border-b border-border">
					<tr class="bg-surface-raised" :class="{ 'text-danger': report.error }">
						<th scope="rowgroup" colspan="5" class="max-w-0 py-1.5 pr-3 pl-2 text-left font-normal">
							<button
								type="button"
								class="flex w-full min-w-0 items-center gap-1 rounded text-left focus-visible:outline-2 focus-visible:outline-accent"
								:aria-expanded="!collapsed.has(report.fileId)"
								:aria-label="
									collapsed.has(report.fileId)
										? t('files.report.expand', { name: reportName(report) })
										: t('files.report.collapse', { name: reportName(report) })
								"
								@click="toggleCollapsed(report.fileId)"
							>
								<i-ph-caret-right class="size-3.5 shrink-0 transition-transform" :class="{ 'rotate-90': !collapsed.has(report.fileId) }" />
								<span class="truncate font-bold">{{ reportName(report) }}</span>
								<span v-if="report.error" class="shrink-0 text-xs">· {{ t("files.report.failed") }}</span>
								<span v-else class="shrink-0 text-xs text-muted">
									· {{ t("files.report.streams", { n: report.streams.length }, report.streams.length) }}
									<template v-if="report.container">· {{ containerLine(report) }}</template>
									<template v-if="report.container && report.container.chapterCount > 0">
										· {{ t("files.report.chapters", { n: report.container.chapterCount }, report.container.chapterCount) }}
									</template>
								</span>
							</button>
						</th>
						<td class="py-1.5 pr-3 text-right align-middle font-mono whitespace-nowrap">
							{{ report.container ? formatGb(report.container.sizeBytes) : "" }}
						</td>
						<td class="py-1.5 pr-4" />
					</tr>
					<tr v-if="report.error && !collapsed.has(report.fileId)">
						<td colspan="7" class="py-2 pr-4 pl-8">
							<p class="text-danger">{{ report.error.message }}</p>
							<template v-if="report.error.ffprobeOutput">
								<button
									type="button"
									class="mt-1 text-xs text-muted underline focus-visible:outline-2 focus-visible:outline-accent"
									:aria-expanded="outputOpen.has(report.fileId)"
									@click="toggleOutput(report.fileId)"
								>
									{{ outputOpen.has(report.fileId) ? t("files.report.hideOutput") : t("files.report.showOutput") }}
								</button>
								<pre
									v-if="outputOpen.has(report.fileId)"
									class="mt-1 max-h-48 overflow-auto rounded bg-surface-sunken p-2 font-mono text-xs whitespace-pre-wrap"
									:aria-label="t('files.report.ffprobeOutput')"
									>{{ report.error.ffprobeOutput }}</pre>
							</template>
						</td>
					</tr>
					<template v-else-if="!collapsed.has(report.fileId)">
						<tr
							v-for="stream in report.streams"
							:key="stream.index"
							class="cursor-pointer"
							:class="rowClass(report.fileId, stream)"
							:aria-selected="isSelected(report.fileId, stream)"
							@click="emit('select', { fileId: report.fileId, index: stream.index })"
						>
							<td :class="cell" class="pl-8">
								<button
									type="button"
									class="font-mono hover:underline focus-visible:outline-2 focus-visible:outline-accent"
									:aria-label="t('files.report.selectStream', { index: stream.index, name: reportName(report) })"
									@click.stop="emit('select', { fileId: report.fileId, index: stream.index })"
								>
									#{{ stream.index }}
								</button>
							</td>
							<td :class="cell">{{ streamTypeName(stream.type) }}</td>
							<td :class="cell" class="font-mono" :title="stream.codecLongName || undefined">{{ stream.codec }}</td>
							<td :class="cell" class="font-mono">{{ stream.language }}</td>
							<td :class="cell" class="max-w-0 min-w-24 truncate" :title="stream.title || undefined">{{ stream.title }}</td>
							<td :class="cell" class="text-right font-mono whitespace-nowrap" :title="sizeTitle(stream)">
								{{ formatStreamSize(stream) }}
							</td>
							<td class="py-1 pr-4 align-middle text-xs text-muted">{{ stream.flags.join(", ") }}</td>
						</tr>
					</template>
				</tbody>
			</template>

			<!-- Flat: one table, the file in every row, failures at the end -->
			<tbody v-else>
				<tr
					v-for="{ report, stream } in flatRows"
					:key="`${report.fileId}:${stream.index}`"
					class="cursor-pointer border-b border-border"
					:class="rowClass(report.fileId, stream)"
					:aria-selected="isSelected(report.fileId, stream)"
					@click="emit('select', { fileId: report.fileId, index: stream.index })"
				>
					<td :class="cell" class="max-w-0 pl-4">
						<button
							type="button"
							class="flex w-full min-w-0 gap-2 text-left hover:underline focus-visible:outline-2 focus-visible:outline-accent"
							:aria-label="t('files.report.selectStream', { index: stream.index, name: reportName(report) })"
							@click.stop="emit('select', { fileId: report.fileId, index: stream.index })"
						>
							<span class="truncate">{{ reportName(report) }}</span>
							<span class="shrink-0 font-mono">#{{ stream.index }}</span>
						</button>
					</td>
					<td :class="cell">{{ streamTypeName(stream.type) }}</td>
					<td :class="cell" class="font-mono" :title="stream.codecLongName || undefined">{{ stream.codec }}</td>
					<td :class="cell" class="font-mono">{{ stream.language }}</td>
					<td :class="cell" class="max-w-0 min-w-24 truncate" :title="stream.title || undefined">{{ stream.title }}</td>
					<td :class="cell" class="text-right font-mono whitespace-nowrap" :title="sizeTitle(stream)">{{ formatStreamSize(stream) }}</td>
					<td class="py-1 pr-4 align-middle text-xs text-muted">{{ stream.flags.join(", ") }}</td>
				</tr>
				<tr v-for="report in failed" :key="report.fileId" class="border-b border-border text-danger">
					<td :class="cell" class="max-w-0 truncate pl-4 font-bold" :title="reportName(report)">{{ reportName(report) }}</td>
					<td colspan="6" class="py-1 pr-4 align-middle">{{ report.error?.message }}</td>
				</tr>
			</tbody>

			<tbody v-if="pendingIds.length > 0">
				<tr class="border-b border-border text-muted">
					<td colspan="7" class="py-1.5 pr-4 pl-4 text-xs">{{ t("files.report.pending", { n: pendingIds.length }, pendingIds.length) }}</td>
				</tr>
			</tbody>
		</table>
	</section>
</template>
