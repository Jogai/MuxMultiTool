<script setup lang="ts">
import IconHelp from "~icons/lucide/circle-question-mark";
import IconMoon from "~icons/lucide/moon";
import IconPlay from "~icons/lucide/play";
import IconSun from "~icons/lucide/sun";

import type { InfoDialogKind } from "~/core/components/ShellInfoDialog.vue";

/** Top bar from the sketches: app name, breadcrumb, job indicator, theme toggle, help and the ☰ menu. */
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
const info = ref<InfoDialogKind>();
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
				<IconPlay />
				<span>{{ jobs.isActive ? t("topBar.jobs.active", { running: jobs.running, queued: jobs.queued }) : t("topBar.jobs.idle") }}</span>
			</RouterLink>
			<ShellIconButton :label="themeLabel" :disabled="!canToggle" @click="toggleTheme()">
				<IconSun v-if="themeVariant === 'dark'" />
				<IconMoon v-else />
			</ShellIconButton>
			<ShellIconButton :label="t('topBar.help')" @click="info = 'help'">
				<IconHelp />
			</ShellIconButton>
			<ShellMenu @select="(kind) => (info = kind)" />
		</div>

		<ShellInfoDialog v-model="info" />
	</header>
</template>
