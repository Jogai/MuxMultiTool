<script setup lang="ts">
/** Status bar: backend state and version, the ffmpeg build (TASK-12) and the mode. */
const { t } = useI18n();
const system = useSystemStore();
const mode = appMode();
</script>

<template>
	<footer class="flex h-7 items-center gap-4 border-t border-border bg-surface-raised px-3 text-xs text-muted">
		<span v-if="system.state === 'loading'">{{ t("statusBar.backend.loading") }}</span>
		<span v-else-if="system.state === 'ok'">{{ t("statusBar.backend.ok", { version: system.version }) }}</span>
		<RouterLink v-else to="/settings" class="text-danger underline">{{ t("statusBar.backend.error") }}</RouterLink>
		<span>{{ t("statusBar.ffmpeg") }}</span>
		<span class="ml-auto">{{ t(`statusBar.mode.${mode}`) }}</span>
	</footer>
</template>
