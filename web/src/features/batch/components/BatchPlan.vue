<script setup lang="ts">
import type { PlanRow } from "../lib/batch";
import { toggled } from "~/core/libs/collections";

/**
 * The dry run's table of the sketch: every matching file with its output name and result (the
 * streams it keeps, or why it is skipped), each row opening on the resolved streams, the container's
 * warnings and the exact command, as the Streams screen shows them for one file.
 */
const {
	rows,
	ready,
	date,
	stale = false
} = defineProps<{
	rows: readonly PlanRow[];
	ready: number;
	date: string;
	/** The form changed since the plan was made. */
	stale?: boolean;
}>();
const { t } = useI18n();
const open = ref(new Set<string>());

const toggle = (id: string) => (open.value = toggled(open.value, id));

const cell = "py-1 pr-3 align-top";
</script>

<template>
	<section
		class="flex flex-col gap-2"
		:aria-label="t('batch.plan.title', { ready, total: rows.length }, rows.length)"
		:class="{ 'opacity-60': stale }"
	>
		<div class="flex flex-wrap items-baseline gap-x-4 gap-y-1">
			<h3 class="font-bold">{{ t("batch.plan.title", { ready, total: rows.length }, rows.length) }}</h3>
			<span class="text-sm text-muted">{{ t("batch.plan.date", { date }) }}</span>
			<span v-if="stale" class="text-sm text-warning" role="status">{{ t("batch.plan.stale") }}</span>
		</div>
		<p v-if="rows.length === 0" class="text-sm text-muted">{{ t("batch.plan.noFiles") }}</p>
		<div v-else class="overflow-x-auto rounded-md border border-border">
			<table class="w-full border-collapse text-sm">
				<thead class="text-left text-xs text-muted uppercase">
					<tr class="border-b border-border">
						<th scope="col" class="w-8 py-2 pr-1 pl-2">
							<span class="sr-only">{{ t("batch.plan.columns.result") }}</span>
						</th>
						<th scope="col" class="py-2 pr-3 font-normal">{{ t("batch.plan.columns.input") }}</th>
						<th scope="col" class="py-2 pr-3 font-normal">{{ t("batch.plan.columns.output") }}</th>
						<th scope="col" class="w-full py-2 pr-2 font-normal">{{ t("batch.plan.columns.result") }}</th>
					</tr>
				</thead>
				<tbody>
					<template v-for="row in rows" :key="row.id">
						<tr class="border-b border-border" :class="{ 'text-muted': !row.ready }">
							<td class="py-1 pr-1 pl-2 align-top">
								<button
									v-if="row.ready"
									type="button"
									class="rounded-md p-0.5 hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent"
									:aria-expanded="open.has(row.id)"
									:aria-label="open.has(row.id) ? t('batch.plan.hide', { name: row.input }) : t('batch.plan.show', { name: row.input })"
									@click="toggle(row.id)"
								>
									<i-ph-caret-down v-if="open.has(row.id)" class="size-4" aria-hidden="true" />
									<i-ph-caret-right v-else class="size-4" aria-hidden="true" />
								</button>
							</td>
							<td :class="cell" class="font-mono whitespace-nowrap" :title="row.inputPath || undefined">{{ row.input }}</td>
							<td :class="cell" class="font-mono whitespace-nowrap">{{ row.outputName }}</td>
							<td class="py-1 pr-2 align-top">
								<template v-if="row.ready">
									<span>{{ t("batch.plan.streamsKept", { kept: row.kept, total: row.total }) }}</span>
									<span v-if="row.container" class="text-muted"> · {{ row.container }}</span>
									<span v-if="row.overwrites" class="text-warning"> · {{ t("batch.plan.overwrites") }}</span>
									<span v-if="row.warnings > 0" class="text-warning">
										· {{ t("batch.plan.warnings", { n: row.warnings }, row.warnings) }}</span
									>
								</template>
								<span v-else class="text-warning">{{ t("batch.plan.skipped") }} · {{ row.skip }}</span>
							</td>
						</tr>
						<tr v-if="row.ready && open.has(row.id)" class="border-b border-border bg-surface-sunken/40">
							<td></td>
							<td colspan="3" class="py-2 pr-2">
								<div class="flex flex-col gap-3">
									<StreamsTable :rows="row.streams" :file-name="row.input" readonly class="rounded-md border border-border bg-surface" />
									<StreamsNotices :issues="row.issues" :warnings="row.selectionWarnings" />
									<StreamsCommand :command="row.command" :title="t('batch.plan.command', { name: row.input })" />
								</div>
							</td>
						</tr>
					</template>
				</tbody>
			</table>
		</div>
	</section>
</template>
