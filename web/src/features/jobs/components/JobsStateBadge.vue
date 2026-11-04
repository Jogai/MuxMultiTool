<script setup lang="ts">
import { JobState } from "~/generated/muxmultitool/jobs/v1/jobs_pb";

/** The state of a job as the sketch draws it: a glyph and a word, coloured by outcome. */
const { state } = defineProps<{ state: JobState }>();
const { t } = useI18n();
const key = computed(() => JobState[state].toLowerCase());
const tone = computed(() => {
	switch (state) {
		case JobState.RUNNING:
			return "text-accent";
		case JobState.SUCCEEDED:
			return "text-success";
		case JobState.FAILED:
			return "text-danger";
		case JobState.CANCELLED:
			return "text-warning";
		default:
			return "text-muted";
	}
});
</script>

<template>
	<span class="inline-flex items-center gap-1.5 whitespace-nowrap" :class="tone">
		<i-ph-record v-if="state === JobState.RUNNING" class="size-3.5 animate-pulse" aria-hidden="true" />
		<i-ph-check v-else-if="state === JobState.SUCCEEDED" class="size-3.5" aria-hidden="true" />
		<i-ph-x v-else-if="state === JobState.FAILED" class="size-3.5" aria-hidden="true" />
		<i-ph-minus v-else-if="state === JobState.CANCELLED" class="size-3.5" aria-hidden="true" />
		<i-ph-circle v-else class="size-3.5" aria-hidden="true" />
		<span>{{ t(`jobs.state.${key}`) }}</span>
	</span>
</template>
