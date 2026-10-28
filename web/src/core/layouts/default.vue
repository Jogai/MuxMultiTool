<script setup lang="ts">
import { navNameForPath } from "~/core/libs/navigation";

/** The app shell from the GUI sketches: top bar, nav rail, the page, status bar. */
const { t } = useI18n();
const route = useRoute();
const current = computed(() => navNameForPath(route.path));
const pageTitle = computed(() => (current.value ? t(`nav.${current.value}`) : t("app.name")));
useTitle(computed(() => (current.value ? `${pageTitle.value} · ${t("app.name")}` : t("app.name"))));
</script>

<template>
	<RekaTooltipProvider :delay-duration="300">
		<div class="grid h-screen grid-cols-[auto_1fr] grid-rows-[auto_1fr_auto] bg-surface text-text">
			<h1 class="sr-only">{{ t("app.name") }}</h1>
			<ShellTopBar class="col-span-2" :title="pageTitle" />
			<ShellNavRail :current="current" />
			<main class="overflow-auto">
				<RouterView />
			</main>
			<ShellStatusBar class="col-span-2" />
		</div>
		<FilePickerDialog />
	</RekaTooltipProvider>
</template>
