<script setup lang="ts">
/** Top bar from the sketches: app name, breadcrumb, job indicator and theme toggle. Help and About live in Settings. */
const { title } = defineProps<{ title: string }>();
const { t } = useI18n();
const jobs = useJobsStore();
const { variant: themeVariant, canToggle, familyName, toggle: toggleTheme } = useTheme();
const themeLabel = computed(() => {
	if (!canToggle.value) {
		return t("topBar.theme.noToggle", { family: familyName.value });
	}
	return themeVariant.value === "dark" ? t("topBar.theme.toLight") : t("topBar.theme.toDark");
});
</script>

<template>
	<header class="flex h-12 items-center gap-3 border-b border-border bg-surface-raised px-3">
		<RouterLink to="/" class="font-bold">{{ t("app.name") }}</RouterLink>
		<nav :aria-label="t('topBar.breadcrumb')" class="truncate text-muted">
			<span aria-hidden="true">›</span>
			{{ title }}
		</nav>

		<div class="ml-auto flex items-center gap-1">
			<RouterLink
				to="/jobs"
				class="flex h-9 items-center gap-2 rounded-md px-2 text-sm hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent"
				:class="jobs.isActive ? 'text-accent' : 'text-muted'"
				:aria-label="t('topBar.jobs.open')"
			>
				<i-ph-play />
				<span>{{ jobs.isActive ? t("topBar.jobs.active", { running: jobs.running, queued: jobs.queued }) : t("topBar.jobs.idle") }}</span>
			</RouterLink>
			<ShellIconButton :label="themeLabel" :disabled="!canToggle" @click="toggleTheme()">
				<i-ph-sun v-if="themeVariant === 'dark'" />
				<i-ph-moon v-else />
			</ShellIconButton>
		</div>
	</header>
</template>
