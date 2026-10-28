<script setup lang="ts">
import { type Job, JobState } from "~/generated/muxmultitool/jobs/v1/jobs_pb";

/**
 * The queue as a table, as in the sketch: status, name, progress bar, speed and ETA per row, with the
 * actions a job's state allows. Clicking a name opens its details.
 */
const { jobs, selectedId } = defineProps<{ jobs: Job[]; selectedId?: string }>();
const emit = defineEmits<{ select: [id: string]; cancel: [job: Job] }>();
const { t } = useI18n();
const now = useClock();

const isActive = (job: Job) => job.state === JobState.RUNNING || job.state === JobState.QUEUED;

/** The share done: 100 once succeeded, the last report otherwise. */
function percent(job: Job): number | undefined {
	return job.state === JobState.SUCCEEDED ? 100 : job.progress?.percent;
}

/** ETA while running, time taken once finished. */
function timing(job: Job): string {
	if (job.state === JobState.RUNNING) {
		return job.progress?.etaMs === undefined ? t("jobs.none") : formatSpan(job.progress.etaMs);
	}
	const elapsed = elapsedMs(job, now.value.getTime());
	return elapsed === undefined ? t("jobs.none") : t("jobs.took", { time: formatSpan(elapsed) });
}
</script>

<template>
	<table class="w-full border-collapse text-sm">
		<thead class="text-left text-xs text-muted uppercase">
			<tr class="border-b border-border">
				<th scope="col" class="py-2 pr-3 font-normal">{{ t("jobs.columns.status") }}</th>
				<th scope="col" class="w-full py-2 pr-3 font-normal">{{ t("jobs.columns.job") }}</th>
				<th scope="col" class="min-w-48 py-2 pr-3 font-normal">{{ t("jobs.columns.progress") }}</th>
				<th scope="col" class="py-2 pr-3 font-normal">{{ t("jobs.columns.speed") }}</th>
				<th scope="col" class="py-2 pr-3 font-normal">{{ t("jobs.columns.eta") }}</th>
				<th scope="col" class="py-2 text-right font-normal">
					<span class="sr-only">{{ t("jobs.columns.actions") }}</span>
				</th>
			</tr>
		</thead>
		<tbody>
			<tr
				v-for="job in jobs"
				:key="job.id"
				class="border-b border-border"
				:class="job.id === selectedId ? 'bg-accent-surface' : 'hover:bg-surface-raised'"
				:aria-selected="job.id === selectedId"
			>
				<td class="py-2 pr-3 align-middle">
					<JobsStateBadge :state="job.state" />
				</td>
				<td class="max-w-0 py-2 pr-3 align-middle">
					<button
						type="button"
						class="block w-full truncate text-left hover:underline focus-visible:outline-2 focus-visible:outline-accent"
						:title="job.name || job.id"
						@click="emit('select', job.id)"
					>
						{{ job.name || job.id }}
					</button>
				</td>
				<td class="py-2 pr-3 align-middle">
					<div v-if="percent(job) !== undefined" class="flex items-center gap-2">
						<div
							class="h-2 flex-1 overflow-hidden rounded bg-surface-sunken"
							role="progressbar"
							:aria-valuenow="Math.round(percent(job) ?? 0)"
							aria-valuemin="0"
							aria-valuemax="100"
							:aria-label="t('jobs.columns.progress')"
						>
							<div
								class="h-full rounded"
								:class="job.state === JobState.FAILED ? 'bg-danger' : job.state === JobState.CANCELLED ? 'bg-warning' : 'bg-accent'"
								:style="{ width: `${Math.min(100, Math.max(0, percent(job) ?? 0))}%` }"
							/>
						</div>
						<span class="w-10 text-right font-mono text-xs">{{ formatPercent(percent(job)) }}</span>
					</div>
					<span v-else-if="job.state === JobState.RUNNING && job.progress" class="font-mono text-xs text-muted">
						{{ t("jobs.written", { time: formatSpan(job.progress.outTimeMs) }) }}
					</span>
					<span v-else class="text-muted">{{ t("jobs.none") }}</span>
				</td>
				<td class="py-2 pr-3 align-middle font-mono whitespace-nowrap">
					{{ job.state === JobState.RUNNING ? (formatSpeed(job.progress?.speed) ?? t("jobs.none")) : t("jobs.none") }}
				</td>
				<td class="py-2 pr-3 align-middle font-mono whitespace-nowrap" :class="{ 'text-muted': job.state !== JobState.RUNNING }">
					{{ timing(job) }}
				</td>
				<td class="py-2 text-right align-middle whitespace-nowrap">
					<button
						v-if="isActive(job)"
						type="button"
						class="rounded-md border border-border px-2 py-1 text-xs hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent"
						:aria-label="t('jobs.actions.cancelJob', { name: job.name || job.id })"
						@click="emit('cancel', job)"
					>
						{{ t("jobs.actions.cancel") }}
					</button>
					<button
						v-else
						type="button"
						class="rounded-md border border-border px-2 py-1 text-xs hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent"
						:class="{ 'text-danger': job.state === JobState.FAILED }"
						@click="emit('select', job.id)"
					>
						{{ job.state === JobState.FAILED ? t("jobs.actions.showError") : t("jobs.actions.details") }}
					</button>
				</td>
			</tr>
		</tbody>
	</table>
</template>
