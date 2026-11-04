<script setup lang="ts">
import type { Component } from "vue";

import IconBatch from "~icons/lucide/layout-grid";
import IconCombine from "~icons/lucide/combine";
import IconCrop from "~icons/lucide/crop";
import IconCut from "~icons/lucide/scissors";
import IconFiles from "~icons/lucide/folder";
import IconImages from "~icons/lucide/image";
import IconJobs from "~icons/lucide/play";
import IconMerge from "~icons/lucide/merge";
import IconPresets from "~icons/lucide/star";
import IconSettings from "~icons/lucide/settings";
import IconStreams from "~icons/lucide/list";

import { NAV_ENTRIES, type NavName } from "~/core/libs/navigation";

/** The nav rail: one entry per screen, the current one highlighted. */
const { current } = defineProps<{ current: NavName | undefined }>();
const { t } = useI18n();

const icons: Record<NavName, Component> = {
	files: IconFiles,
	streams: IconStreams,
	combine: IconCombine,
	cut: IconCut,
	merge: IconMerge,
	crop: IconCrop,
	images: IconImages,
	batch: IconBatch,
	presets: IconPresets,
	jobs: IconJobs,
	settings: IconSettings
};
</script>

<template>
	<nav :aria-label="t('nav.label')" class="flex w-20 flex-col gap-1 overflow-y-auto border-r border-border bg-surface-raised p-2">
		<RouterLink
			v-for="entry in NAV_ENTRIES"
			:key="entry.name"
			:to="entry.path"
			class="flex flex-col items-center gap-1 rounded-md px-1 py-2 text-xs focus-visible:outline-2 focus-visible:outline-accent"
			:class="entry.name === current ? 'bg-accent-surface text-accent' : 'text-muted hover:bg-surface-sunken hover:text-text'"
			:aria-current="entry.name === current ? 'page' : undefined"
		>
			<component :is="icons[entry.name]" class="size-5" />
			<span>{{ t(`nav.${entry.name}`) }}</span>
		</RouterLink>
	</nav>
</template>
