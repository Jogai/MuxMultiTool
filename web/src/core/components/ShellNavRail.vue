<script setup lang="ts">
import { NAV_ENTRIES, type NavName } from "~/core/libs/navigation";

/** The nav rail: one entry per screen, the current one highlighted. */
const { current } = defineProps<{ current: NavName | undefined }>();
const { t } = useI18n();
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
			<!-- One tag per screen, so the icons resolve as components at build time -->
			<i-ph-folder v-if="entry.name === 'files'" class="size-5" />
			<i-ph-list-bullets v-else-if="entry.name === 'streams'" class="size-5" />
			<i-mmt-combine v-else-if="entry.name === 'combine'" class="size-5" />
			<i-ph-scissors v-else-if="entry.name === 'cut'" class="size-5" />
			<i-ph-arrows-merge v-else-if="entry.name === 'merge'" class="size-5" />
			<i-ph-crop v-else-if="entry.name === 'crop'" class="size-5" />
			<i-ph-image v-else-if="entry.name === 'images'" class="size-5" />
			<i-ph-squares-four v-else-if="entry.name === 'batch'" class="size-5" />
			<i-ph-star v-else-if="entry.name === 'presets'" class="size-5" />
			<i-ph-play v-else-if="entry.name === 'jobs'" class="size-5" />
			<i-ph-gear v-else-if="entry.name === 'settings'" class="size-5" />
			<span>{{ t(`nav.${entry.name}`) }}</span>
		</RouterLink>
	</nav>
</template>
