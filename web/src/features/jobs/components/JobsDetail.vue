<script setup lang="ts">
import IconX from "~icons/lucide/x";

import { type Job, JobState } from "~/generated/muxmultitool/jobs/v1/jobs_pb";

/**
 * One job in full, below the table as in the sketch: the live figures, then tabs for the readable error
 * with the raw ffmpeg output, the scrollable log and the exact command.
 */
const { job } = defineProps<{ job: Job }>();
const emit = defineEmits<{ close: []; cancel: [job: Job] }>();
const { t } = useI18n();
const now = useClock();

type Tab = "error" | "log" | "command";
const tab = ref<Tab>("log");
const failed = computed(() => job.state === JobState.FAILED);
const isActive = computed(() => job.state === JobState.RUNNING || job.state === JobState.QUEUED);
const elapsed = computed(() => elapsedMs(job, now.value.getTime()));
const percent = computed(() => (job.state === JobState.SUCCEEDED ? 100 : job.progress?.percent));

// A failed job opens on its error; any other on the log.
watch(
	() => [job.id, failed.value] as const,
	([, isFailed]) => {
		tab.value = isFailed ? "error" : "log";
	},
	{ immediate: true }
);

const figures = computed(() => [
	{ key: "progress", value: formatPercent(percent.value) },
	{ key: "eta", value: job.state === JobState.RUNNING && job.progress?.etaMs !== undefined ? formatSpan(job.progress.etaMs) : undefined },
	{ key: "speed", value: job.state === JobState.RUNNING ? formatSpeed(job.progress?.speed) : undefined },
	{
		key: "fps",
		value: job.state === JobState.RUNNING && job.progress && job.progress.fps > 0 ? Math.round(job.progress.fps).toString() : undefined
	},
	{ key: "written", value: job.progress && job.progress.totalSize > 0n ? formatBytes(job.progress.totalSize) : undefined },
	{ key: "elapsed", value: elapsed.value === undefined ? undefined : formatSpan(elapsed.value) }
]);

// The log follows the newest line until the reader scrolls up.
const logEl = ref<HTMLElement>();
const follow = ref(true);
function onLogScroll() {
	const el = logEl.value;
	if (el) {
		follow.value = el.scrollHeight - el.scrollTop - el.clientHeight < 8;
	}
}
function scrollLog() {
	if (follow.value) {
		logEl.value?.scrollTo({ top: logEl.value.scrollHeight });
	}
}
watch(() => [job.id, tab.value, job.log.length] as const, scrollLog, { flush: "post" });
watch(
	() => job.id,
	() => {
		follow.value = true;
	}
);

// The panel sits below the table; bring it into view when a job is chosen.
const root = ref<HTMLElement>();
const reveal = () => root.value?.scrollIntoView({ block: "nearest", behavior: "smooth" });
onMounted(() => {
	reveal();
	scrollLog();
});
watch(() => job.id, reveal, { flush: "post" });

const logText = computed(() => job.log.join("\n"));
const commandClipboard = useClipboard();
const logClipboard = useClipboard();
</script>

<template>
	<section ref="root" class="flex min-h-0 flex-col rounded-lg border border-border bg-surface-raised" :aria-label="job.name || job.id">
		<header class="flex items-center gap-3 border-b border-border px-4 py-3">
			<JobsStateBadge :state="job.state" />
			<h3 class="min-w-0 truncate font-bold" :title="job.name || job.id">{{ job.name || job.id }}</h3>
			<div class="ml-auto flex items-center gap-1">
				<button
					v-if="isActive"
					type="button"
					class="rounded-md border border-border px-2 py-1 text-xs hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent"
					@click="emit('cancel', job)"
				>
					{{ t("jobs.actions.cancel") }}
				</button>
				<button
					type="button"
					class="flex size-8 items-center justify-center rounded-md text-muted hover:bg-surface-sunken hover:text-text focus-visible:outline-2 focus-visible:outline-accent"
					:aria-label="t('jobs.actions.close')"
					@click="emit('close')"
				>
					<IconX class="size-4" />
				</button>
			</div>
		</header>

		<dl class="grid grid-cols-3 gap-x-6 gap-y-2 px-4 py-3 text-sm lg:grid-cols-6">
			<div v-for="figure in figures" :key="figure.key">
				<dt class="text-xs text-muted">{{ t(`jobs.detail.${figure.key}`) }}</dt>
				<dd class="font-mono">{{ figure.value ?? t("jobs.none") }}</dd>
			</div>
		</dl>

		<RekaTabsRoot v-model="tab" class="flex min-h-0 flex-col">
			<RekaTabsList class="flex gap-1 border-b border-border px-4">
				<RekaTabsTrigger
					v-if="failed"
					value="error"
					class="-mb-px border-b-2 border-transparent px-2 py-2 text-sm text-danger data-[state=active]:border-danger focus-visible:outline-2 focus-visible:outline-accent"
				>
					{{ t("jobs.detail.tabs.error") }}
				</RekaTabsTrigger>
				<RekaTabsTrigger
					value="log"
					class="-mb-px border-b-2 border-transparent px-2 py-2 text-sm text-muted data-[state=active]:border-accent data-[state=active]:text-text focus-visible:outline-2 focus-visible:outline-accent"
				>
					{{ t("jobs.detail.tabs.log") }}
				</RekaTabsTrigger>
				<RekaTabsTrigger
					value="command"
					class="-mb-px border-b-2 border-transparent px-2 py-2 text-sm text-muted data-[state=active]:border-accent data-[state=active]:text-text focus-visible:outline-2 focus-visible:outline-accent"
				>
					{{ t("jobs.detail.tabs.command") }}
				</RekaTabsTrigger>
			</RekaTabsList>

			<RekaTabsContent v-if="failed" value="error" class="p-4 text-sm">
				<p class="font-bold text-danger">{{ job.error?.message }}</p>
				<h4 class="mt-4 text-xs text-muted">{{ t("jobs.detail.rawOutput") }}</h4>
				<pre
					v-if="job.error?.ffmpegOutput"
					class="mt-1 max-h-64 overflow-auto rounded bg-surface-sunken p-2 font-mono text-xs whitespace-pre-wrap"
					:aria-label="t('jobs.detail.rawOutput')"
					>{{ job.error.ffmpegOutput }}</pre>
				<p v-else class="mt-1 text-muted">{{ t("jobs.detail.noRawOutput") }}</p>
			</RekaTabsContent>

			<RekaTabsContent value="log" class="flex min-h-0 flex-col p-4 text-sm">
				<pre
					v-if="job.log.length > 0"
					ref="logEl"
					class="max-h-80 overflow-auto rounded bg-surface-sunken p-2 font-mono text-xs whitespace-pre-wrap"
					:aria-label="t('jobs.detail.log')"
					@scroll.passive="onLogScroll"
					>{{ logText }}</pre>
				<p v-else class="text-muted">{{ t("jobs.detail.logEmpty") }}</p>
				<div class="mt-3 flex justify-end">
					<button
						type="button"
						class="rounded-md border border-border px-2 py-1 text-xs hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent"
						:disabled="job.log.length === 0"
						@click="logClipboard.copy(logText)"
					>
						{{ logClipboard.copied.value ? t("jobs.actions.copied") : t("jobs.actions.copyLog") }}
					</button>
				</div>
			</RekaTabsContent>

			<RekaTabsContent value="command" class="p-4 text-sm">
				<p class="text-xs text-muted">{{ t("jobs.detail.command") }}</p>
				<pre v-if="job.command" class="mt-1 overflow-auto rounded bg-surface-sunken p-2 font-mono text-xs whitespace-pre-wrap">{{
					job.command
				}}</pre>
				<p v-else class="mt-1 text-muted">{{ t("jobs.detail.commandPending") }}</p>
				<details v-if="job.argv.length > 0" class="mt-3">
					<summary class="cursor-pointer text-xs text-muted">{{ t("jobs.detail.argv") }}</summary>
					<ol class="mt-1 font-mono text-xs">
						<li v-for="(arg, index) in job.argv" :key="index" class="break-all">{{ arg }}</li>
					</ol>
				</details>
				<div class="mt-3 flex justify-end">
					<button
						type="button"
						class="rounded-md border border-border px-2 py-1 text-xs hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent"
						:disabled="!job.command"
						@click="commandClipboard.copy(job.command)"
					>
						{{ commandClipboard.copied.value ? t("jobs.actions.copied") : t("jobs.actions.copyCommand") }}
					</button>
				</div>
			</RekaTabsContent>
		</RekaTabsRoot>
	</section>
</template>
