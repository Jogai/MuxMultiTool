<script setup lang="ts">
import type { AppError } from "~/core/libs/errors";
import { type Job, JobState } from "~/generated/muxmultitool/jobs/v1/jobs_pb";

/**
 * Asks before a job is cancelled: the model is the job in question, `undefined` when closed. Confirming
 * calls the store; the stream (and the answer) move the job to `cancelled`, and a refusal stays visible
 * in the dialog.
 */
const job = defineModel<Job>();
const { t } = useI18n();
const jobs = useJobsStore();
const busy = ref(false);
const error = ref<AppError>();

function close() {
	job.value = undefined;
	error.value = undefined;
}

async function confirm() {
	if (!job.value) {
		return;
	}
	busy.value = true;
	error.value = await jobs.cancel(job.value.id);
	busy.value = false;
	if (!error.value) {
		close();
	}
}
</script>

<template>
	<RekaAlertDialogRoot :open="job !== undefined" @update:open="(value) => !value && close()">
		<RekaAlertDialogPortal>
			<RekaAlertDialogOverlay class="fixed inset-0 bg-overlay" />
			<RekaAlertDialogContent
				class="fixed top-1/2 left-1/2 w-[min(28rem,calc(100vw-2rem))] -translate-1/2 rounded-lg border border-border bg-surface p-5 shadow-lg"
			>
				<RekaAlertDialogTitle class="text-lg font-bold">{{ t("jobs.cancelDialog.title") }}</RekaAlertDialogTitle>
				<RekaAlertDialogDescription v-if="job" class="mt-2 text-sm text-muted">
					{{ t(job.state === JobState.RUNNING ? "jobs.cancelDialog.running" : "jobs.cancelDialog.queued", { name: job.name || job.id }) }}
				</RekaAlertDialogDescription>
				<ErrorPanel v-if="error" :error="error" class="mt-3" />
				<div class="mt-5 flex justify-end gap-2">
					<RekaAlertDialogCancel
						class="rounded-md border border-border px-3 py-1.5 text-sm hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent"
					>
						{{ t("jobs.cancelDialog.keep") }}
					</RekaAlertDialogCancel>
					<button
						type="button"
						class="rounded-md bg-danger px-3 py-1.5 text-sm text-on-accent focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-60"
						:disabled="busy"
						@click="confirm"
					>
						{{ t("jobs.cancelDialog.confirm") }}
					</button>
				</div>
			</RekaAlertDialogContent>
		</RekaAlertDialogPortal>
	</RekaAlertDialogRoot>
</template>
