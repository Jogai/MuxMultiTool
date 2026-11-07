<script setup lang="ts">
/** Status bar: backend state and version, the ffmpeg build (TASK-12) and the mode. */
const { t } = useI18n();
const system = useSystemStore();
const mode = appMode();
const ffmpeg = useFfmpegInfo();
onMounted(() => void ffmpeg.load());
</script>

<template>
	<footer class="flex h-7 items-center gap-4 border-t border-border bg-surface-raised px-3 text-xs text-muted">
		<span v-if="system.state === 'loading'">{{ t("statusBar.backend.loading") }}</span>
		<span v-else-if="system.state === 'ok'">{{ t("statusBar.backend.ok", { version: system.version }) }}</span>
		<span v-else class="text-danger">{{ t("statusBar.backend.error") }}</span>
		<span v-if="ffmpeg.state.value === 'ok'">{{ t("statusBar.ffmpeg.ok", { version: ffmpeg.version.value }) }}</span>
		<RouterLink
			v-else-if="ffmpeg.state.value === 'unavailable' || ffmpeg.state.value === 'error'"
			to="/settings"
			class="text-danger underline"
			>{{ t("statusBar.ffmpeg.missing") }}</RouterLink
		>
		<span v-else>{{ t("statusBar.ffmpeg.loading") }}</span>
		<span class="ml-auto">{{ t(`statusBar.mode.${mode}`) }}</span>
	</footer>
</template>
