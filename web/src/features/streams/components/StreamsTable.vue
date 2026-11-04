<script setup lang="ts">
import { type SelectionRow, totals } from "../lib/streams";
import { formatGb, formatStreamSize, streamTypeName } from "~/core/libs/reportFormat";

/** A row of the table: the selection row plus what the issues and the extraction say about it. */
export interface TableRow extends SelectionRow {
	/** The file the stream is written to, when extracting. */
	output?: string;
	/** Problems the target container has with the stream, or why it has no file of its own. */
	issues: readonly { blocking: boolean; message: string }[];
}

/**
 * The per-file selection table of the sketch: a Keep tick per stream, the stream as the report shows it,
 * and a "why" column that names the rule which decided the stream, so a surprising result is easy to
 * trace. Rows ticked by hand are highlighted, and "Reset to rules" puts them back.
 */
const {
	rows,
	fileName,
	loading = false,
	hasOverrides = false,
	extracting = false
} = defineProps<{ rows: readonly TableRow[]; fileName: string; loading?: boolean; hasOverrides?: boolean; extracting?: boolean }>();
const emit = defineEmits<{ toggle: [index: number]; reset: [] }>();
const { t } = useI18n();

const summary = computed(() => totals(rows));
const why = (row: TableRow) => (row.overridden ? t(row.kept ? "streams.table.tickedByHand" : "streams.table.untickedByHand") : row.why);

const cell = "py-1 pr-3 align-middle";
</script>

<template>
	<section class="flex min-w-0 flex-col" :aria-label="t('streams.table.title', { name: fileName })" :aria-busy="loading">
		<div class="overflow-x-auto">
			<table class="w-full border-collapse text-sm">
				<thead class="text-left text-xs text-muted uppercase">
					<tr class="border-b border-border">
						<th scope="col" class="py-2 pr-3 pl-2 font-normal">{{ t("streams.table.keep") }}</th>
						<th scope="col" class="py-2 pr-3 font-normal">#</th>
						<th scope="col" class="py-2 pr-3 font-normal">{{ t("files.report.columns.type") }}</th>
						<th scope="col" class="py-2 pr-3 font-normal">{{ t("files.report.columns.codec") }}</th>
						<th scope="col" class="py-2 pr-3 font-normal">{{ t("files.report.columns.language") }}</th>
						<th scope="col" class="py-2 pr-3 font-normal">{{ t("files.report.columns.title") }}</th>
						<th scope="col" class="py-2 pr-3 text-right font-normal whitespace-nowrap">{{ t("files.report.columns.size") }}</th>
						<th scope="col" class="w-full py-2 pr-2 font-normal">{{ extracting ? t("streams.table.file") : t("streams.table.why") }}</th>
					</tr>
				</thead>
				<tbody>
					<tr
						v-for="row in rows"
						:key="row.stream.index"
						class="border-b border-border"
						:class="{ 'bg-accent-surface': row.overridden, 'text-muted': !row.kept && !row.overridden }"
					>
						<td :class="cell" class="pl-2">
							<input
								type="checkbox"
								class="accent-accent"
								:checked="row.kept"
								:aria-label="t('streams.table.keepStream', { index: row.stream.index })"
								@change="emit('toggle', row.stream.index)"
							/>
						</td>
						<td :class="cell" class="font-mono">{{ row.stream.index }}</td>
						<td :class="cell">{{ streamTypeName(row.stream.type) }}</td>
						<td :class="cell" class="font-mono" :title="row.stream.codecLongName || undefined">{{ row.stream.codec }}</td>
						<td :class="cell" class="font-mono">{{ row.stream.language }}</td>
						<td :class="cell" class="max-w-0 min-w-24 truncate" :title="row.stream.title || undefined">{{ row.stream.title }}</td>
						<td :class="cell" class="text-right font-mono whitespace-nowrap">{{ formatStreamSize(row.stream) }}</td>
						<td class="py-1 pr-2 align-middle text-xs">
							<span v-if="row.output" class="font-mono">{{ row.output }}</span>
							<span v-else class="text-muted">{{ why(row) }}</span>
							<ul v-if="row.issues.length > 0">
								<li v-for="issue in row.issues" :key="issue.message" :class="issue.blocking ? 'text-danger' : 'text-warning'">
									<span aria-hidden="true">{{ issue.blocking ? "✕" : "⚠" }}</span> {{ issue.message }}
								</li>
							</ul>
						</td>
					</tr>
				</tbody>
			</table>
		</div>
		<div class="flex flex-wrap items-center gap-3 px-2 py-2 text-sm">
			<span>
				{{ t("streams.table.result", { kept: summary.kept, total: summary.total }) }}
				<template v-if="!extracting">
					· {{ formatGb(summary.keptBytes) }} GB
					<span v-if="formatGb(summary.droppedBytes) !== '0.00'" class="text-muted">(−{{ formatGb(summary.droppedBytes) }} GB)</span>
				</template>
			</span>
			<span v-if="loading" class="text-muted">{{ t("streams.table.loading") }}</span>
			<button
				v-if="hasOverrides"
				type="button"
				class="ml-auto rounded-md border border-border px-3 py-1 hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent"
				@click="emit('reset')"
			>
				{{ extracting ? t("streams.table.resetAll") : t("streams.table.reset") }}
			</button>
		</div>
	</section>
</template>
