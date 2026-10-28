<script setup lang="ts">
import { defaultDarkVariant } from "~/core/themes";
import type { ThemeFamily } from "~/core/themes/palette";

/** Step 1 of the appearance settings: one card per family, previewed by its dark side. */
const { families } = defineProps<{ families: readonly ThemeFamily[] }>();
const family = defineModel<string>({ required: true });
const { t } = useI18n();
</script>

<template>
	<RekaRadioGroupRoot
		:model-value="family"
		class="grid grid-cols-[repeat(auto-fill,minmax(12rem,1fr))] gap-2"
		@update:model-value="(value) => typeof value === 'string' && (family = value)"
	>
		<RekaRadioGroupItem
			v-for="entry in families"
			:key="entry.id"
			:value="entry.id"
			class="flex items-center gap-3 rounded-md border border-border bg-surface-raised p-2 text-left hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent data-[state=checked]:border-accent data-[state=checked]:bg-accent-surface"
		>
			<ThemeSwatch :colors="defaultDarkVariant(entry).colors" />
			<span class="min-w-0">
				<span class="block truncate font-bold">{{ entry.name }}</span>
				<span class="block text-xs text-muted">{{ t("pages.settings.variants", entry.variants.length) }}</span>
			</span>
		</RekaRadioGroupItem>
	</RekaRadioGroupRoot>
</template>
