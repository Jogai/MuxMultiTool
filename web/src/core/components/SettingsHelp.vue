<script setup lang="ts">
/** The Help section of Settings: one line per screen, in nav order, and where the shortcuts, the command and the log are. */
const { t } = useI18n();
const SCREENS = ["files", "streams", "combine", "cut", "merge", "crop", "images", "batch", "presets", "jobs", "settings"] as const;
</script>

<template>
	<div class="max-w-prose space-y-4 text-sm">
		<p>{{ t("pages.settings.help.intro") }}</p>
		<dl class="grid grid-cols-[6rem_1fr] gap-x-3 gap-y-1.5">
			<template v-for="screen in SCREENS" :key="screen">
				<dt class="font-bold">{{ t(`nav.${screen}`) }}</dt>
				<dd class="text-muted">{{ t(`pages.settings.help.screens.${screen}`) }}</dd>
			</template>
		</dl>
		<p>
			<i-ph-keyboard class="mr-1 inline size-4 align-text-bottom" aria-hidden="true" />
			{{ t("pages.settings.help.shortcuts") }}
		</p>
		<p>{{ t("pages.settings.help.command") }}</p>
		<p>{{ t("pages.settings.help.log") }}</p>
	</div>
</template>
