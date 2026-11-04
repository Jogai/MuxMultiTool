<script setup lang="ts">
import { downloadUrl, jobOutputPath, siblingFileId } from "../lib/outputs";
import { jobLink } from "~/core/libs/navigation";
import { Code } from "@connectrpc/connect";
import { codeName } from "~/core/libs/errors";
import { appMode } from "~/core/libs/filePicker";
import { formatPercent } from "~/core/libs/jobFormat";
import { showInFolder } from "~/core/libs/showInFolder";
import { type Job, JobState } from "~/generated/muxmultitool/jobs/v1/jobs_pb";

/**
 * What became of a submitted job: its state with a link to the Jobs screen, and once it succeeded a
 * way to the output. In container mode that is a download from `GET /files/{id}` (the output's id is
 * derived from the input's, see lib/outputs); in desktop mode the file is on this machine, so the
 * download is hidden and "show in folder" opens the system file manager instead (TASK-34).
 */
const { job, inputId, output } = defineProps<{
	job: Job;
	/** The input as the operation named it: a file id, or an absolute path in desktop mode. */
	inputId: string;
	/** The output as the operation named it: a bare name next to the input, or an absolute path. */
	output: string;
}>();
const { t } = useI18n();
const mode = appMode();

const stateKey = computed(() => {
	switch (job.state) {
		case JobState.RUNNING:
			return "running";
		case JobState.SUCCEEDED:
			return "succeeded";
		case JobState.FAILED:
			return "failed";
		case JobState.CANCELLED:
			return "cancelled";
		default:
			return "queued";
	}
});
const percent = computed(() => formatPercent(job.progress?.percent) ?? "");
const download = computed(() =>
	mode === "container" && job.state === JobState.SUCCEEDED ? downloadUrl(siblingFileId(inputId, output)) : undefined
);
const revealPath = computed(() => (mode === "desktop" && job.state === JobState.SUCCEEDED ? jobOutputPath(job) : undefined));
const failure = computed(() =>
	job.error
		? {
				code: Code.FailedPrecondition,
				codeName: codeName(Code.FailedPrecondition),
				message: job.error.message,
				raw: job.error.ffmpegOutput || undefined,
				cause: job.error
			}
		: undefined
);
const revealError = ref<string>();

async function reveal(path: string) {
	revealError.value = undefined;
	try {
		await showInFolder(path);
	} catch (cause) {
		revealError.value = cause instanceof Error ? cause.message : String(cause);
	}
}
</script>

<template>
	<div class="flex flex-col gap-2 text-sm" role="status">
		<p class="flex flex-wrap items-center gap-x-3 gap-y-1">
			<span :class="job.state === JobState.FAILED ? 'text-danger' : job.state === JobState.SUCCEEDED ? 'text-success' : ''">
				{{ t(`images.job.${stateKey}`, { name: job.name, percent }) }}
			</span>
			<RouterLink :to="jobLink(job.id)" class="text-accent underline focus-visible:outline-2 focus-visible:outline-accent">
				{{ t("images.job.view") }}
			</RouterLink>
			<a
				v-if="download"
				:href="download"
				download
				class="inline-flex items-center gap-1 rounded-md border border-border px-3 py-1 hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent"
			>
				<i-ph-download-simple class="size-4" aria-hidden="true" />
				{{ t("images.job.download") }}
			</a>
			<button
				v-else-if="revealPath"
				type="button"
				class="inline-flex items-center gap-1 rounded-md border border-border px-3 py-1 hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent"
				@click="reveal(revealPath)"
			>
				<i-ph-folder-open class="size-4" aria-hidden="true" />
				{{ t("images.job.showInFolder") }}
			</button>
		</p>
		<p v-if="revealError" class="text-danger">{{ t("images.job.showFailed", { detail: revealError }) }}</p>
		<ErrorPanel v-if="failure" :error="failure" />
	</div>
</template>
