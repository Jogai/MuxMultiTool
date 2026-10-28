<script setup lang="ts">
import type { Job } from "~/generated/muxmultitool/jobs/v1/jobs_pb";

/**
 * The Jobs screen (sketch 05): the live queue, with the details of the job chosen in the table below it.
 * The chosen job is in the URL (`?job=…`), so a reload or a shared link lands on the same job.
 */
const { t } = useI18n();
const jobs = useJobsStore();
const route = useRoute();
const router = useRouter();

const selectedId = computed(() => (typeof route.query.job === "string" ? route.query.job : undefined));
const selected = computed(() => (selectedId.value === undefined ? undefined : jobs.byId(selectedId.value)));
const cancelling = ref<Job>();

function select(id?: string) {
	void router.replace({ query: { ...route.query, job: id } });
}
</script>

<template>
	<section class="flex flex-col gap-4 p-6">
		<header class="flex flex-wrap items-baseline gap-x-4 gap-y-1">
			<h2 class="text-xl font-bold">{{ t("jobs.title") }}</h2>
			<p class="text-sm text-muted">{{ t("jobs.summary", { running: jobs.running, queued: jobs.queued, finished: jobs.finished }) }}</p>
		</header>

		<JobsConnectionNotice />

		<p v-if="jobs.list.length === 0" class="max-w-prose text-muted">{{ t("jobs.empty") }}</p>
		<JobsTable v-else :jobs="jobs.list" :selected-id="selectedId" @select="select" @cancel="cancelling = $event" />

		<JobsDetail v-if="selected" :job="selected" @close="select()" @cancel="cancelling = $event" />

		<JobsCancelDialog v-model="cancelling" />
	</section>
</template>
