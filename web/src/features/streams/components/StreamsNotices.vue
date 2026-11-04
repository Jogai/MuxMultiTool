<script setup lang="ts">
/**
 * What the backend wants checked before a submit: blocking issues (the job would be refused) marked
 * ✕, non-blocking ones and warnings marked ⚠. Shared by the Streams and Combine screens.
 */
const { issues = [], warnings = [] } = defineProps<{
	issues?: readonly { blocking: boolean; message: string }[];
	warnings?: readonly string[];
}>();
const { t } = useI18n();
const blocking = computed(() => issues.filter((issue) => issue.blocking));
const advisory = computed(() => issues.filter((issue) => !issue.blocking));
</script>

<template>
	<div
		v-if="issues.length > 0 || warnings.length > 0"
		class="flex flex-col gap-1 text-sm"
		role="status"
		:aria-label="t('streams.notices.title')"
	>
		<p v-for="issue in blocking" :key="issue.message" class="text-danger"><span aria-hidden="true">✕</span> {{ issue.message }}</p>
		<p v-for="issue in advisory" :key="issue.message" class="text-warning"><span aria-hidden="true">⚠</span> {{ issue.message }}</p>
		<p v-for="warning in warnings" :key="warning" class="text-warning"><span aria-hidden="true">⚠</span> {{ warning }}</p>
	</div>
</template>
