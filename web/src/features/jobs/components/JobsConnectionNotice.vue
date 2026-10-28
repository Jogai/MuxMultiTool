<script setup lang="ts">
/** Tells when the queue view is not live: still connecting, or dropped and reconnecting with a way to retry at once. */
const { t } = useI18n();
const jobs = useJobsStore();
</script>

<template>
	<div
		v-if="jobs.connection !== 'live'"
		role="status"
		class="flex flex-wrap items-center gap-x-3 gap-y-1 rounded-md border px-3 py-2 text-sm"
		:class="jobs.connection === 'connecting' ? 'border-border text-muted' : 'border-warning text-warning'"
	>
		<span>{{ t(`jobs.connection.${jobs.connection}`) }}</span>
		<span v-if="jobs.connectionError" class="text-muted">{{ jobs.connectionError.message }}</span>
		<button
			v-if="jobs.connection === 'reconnecting'"
			type="button"
			class="ml-auto rounded-md border border-border px-2 py-1 text-xs text-text hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent"
			@click="jobs.retryNow()"
		>
			{{ t("jobs.connection.retry") }}
		</button>
	</div>
</template>
