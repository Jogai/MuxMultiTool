<script setup lang="ts">
import { progressCounts, type ProgressRow } from "../lib/batch";
import type { BatchItem } from "~/generated/muxmultitool/batch/v1/batch_pb";
import { JobState } from "~/generated/muxmultitool/jobs/v1/jobs_pb";
import { jobLink } from "~/features/streams/lib/streams";

/**
 * A queued batch as it runs: a bar for the whole batch, every file with its job's state and share
 * done, the failed files with their errors, and the files the submit skipped with the reason. Each
 * file links to its job on the Jobs screen for the log and the command.
 */
const { rows, skipped = [] } = defineProps<{ rows: readonly ProgressRow[]; skipped?: readonly BatchItem[] }>();
const emit = defineEmits<{ another: [] }>();
const { t } = useI18n();

const counts = computed(() => progressCounts(rows));
const done = computed(() => counts.value.succeeded + counts.value.failed + counts.value.cancelled);
const failures = computed(() => rows.filter((row) => row.state === JobState.FAILED));
const finished = computed(() => done.value === rows.length);

const cell = "py-1 pr-3 align-top";
</script>

<template>
	<section class="flex flex-col gap-3" :aria-label="t('batch.progress.title')">
		<div class="flex flex-wrap items-baseline gap-x-4 gap-y-1">
			<h3 class="font-bold">{{ t("batch.progress.title") }}</h3>
			<span class="text-sm text-muted" role="status">
				{{ t("batch.progress.summary", { done, total: rows.length }) }}
				<span v-if="counts.failed > 0" class="text-danger"> · {{ t("batch.progress.failed", { n: counts.failed }, counts.failed) }}</span>
			</span>
			<button
				v-if="finished"
				type="button"
				class="ml-auto rounded-md border border-border px-3 py-1 text-sm hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent"
				@click="emit('another')"
			>
				{{ t("batch.progress.another") }}
			</button>
		</div>
		<div
			class="h-2 w-full max-w-3xl overflow-hidden rounded-full bg-surface-sunken"
			role="progressbar"
			:aria-valuenow="counts.percent"
			aria-valuemin="0"
			aria-valuemax="100"
			:aria-label="t('batch.progress.title')"
		>
			<div class="h-full bg-accent" :style="{ width: `${counts.percent}%` }"></div>
		</div>

		<div class="overflow-x-auto rounded-md border border-border">
			<table class="w-full border-collapse text-sm">
				<thead class="text-left text-xs text-muted uppercase">
					<tr class="border-b border-border">
						<th scope="col" class="py-2 pr-3 pl-2 font-normal">{{ t("batch.progress.columns.status") }}</th>
						<th scope="col" class="w-full py-2 pr-3 font-normal">{{ t("batch.progress.columns.file") }}</th>
						<th scope="col" class="py-2 pr-2 text-right font-normal">{{ t("batch.progress.columns.progress") }}</th>
					</tr>
				</thead>
				<tbody>
					<tr v-for="row in rows" :key="row.jobId" class="border-b border-border">
						<td :class="cell" class="pl-2"><JobsStateBadge :state="row.state" /></td>
						<td :class="cell">
							<RouterLink :to="jobLink(row.jobId)" class="font-mono text-accent underline" :title="t('batch.progress.view')">{{
								row.name
							}}</RouterLink>
							<p v-if="row.error" class="text-xs text-danger">{{ row.error }}</p>
						</td>
						<td class="py-1 pr-2 text-right align-top font-mono whitespace-nowrap">
							{{ row.percent === undefined ? "" : formatPercent(row.percent) }}
						</td>
					</tr>
				</tbody>
			</table>
		</div>

		<div v-if="failures.length > 0" class="flex flex-col gap-2" :aria-label="t('batch.progress.failures')">
			<h4 class="text-sm font-bold text-danger">{{ t("batch.progress.failures") }}</h4>
			<ul class="flex flex-col gap-2">
				<li v-for="row in failures" :key="row.jobId" class="rounded-md border border-danger/40 p-2 text-sm">
					<RouterLink :to="jobLink(row.jobId)" class="font-mono text-accent underline">{{ row.name }}</RouterLink>
					<span v-if="row.error"> · {{ row.error }}</span>
					<pre v-if="row.ffmpegOutput" class="mt-1 overflow-x-auto rounded-md bg-surface-sunken p-2 text-xs whitespace-pre-wrap">{{
						row.ffmpegOutput
					}}</pre>
				</li>
			</ul>
		</div>

		<div v-if="skipped.length > 0" class="text-sm">
			<p class="text-muted">{{ t("batch.progress.skipped", { n: skipped.length }, skipped.length) }}</p>
			<ul class="mt-1 flex flex-col gap-0.5">
				<li v-for="item in skipped" :key="item.input?.id ?? item.outputName">
					<span class="font-mono">{{ item.input?.name }}</span> <span class="text-muted">· {{ item.skip }}</span>
				</li>
			</ul>
		</div>
	</section>
</template>
