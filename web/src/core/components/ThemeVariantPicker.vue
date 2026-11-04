<script setup lang="ts">
import type { AcceptableValue } from "reka-ui";

import type { ThemeMode, ThemeSetting } from "~/core/libs/theme";
import { findVariant } from "~/core/themes";
import type { ThemeFamily } from "~/core/themes/palette";

/**
 * Step 2 of the appearance settings: the variants of the chosen family. In single mode the cards are radio
 * buttons; in auto mode each card carries a ☀ and a ☾ marker, so the light and dark variant are chosen on
 * the cards themselves, without separate light/dark selectors.
 */
const { family, setting } = defineProps<{ family: ThemeFamily; setting: ThemeSetting }>();
const emit = defineEmits<{ update: [patch: Partial<ThemeSetting>] }>();
const { t } = useI18n();

const hasLightSide = computed(() => family.variants.some((variant) => variant.light) && family.variants.some((variant) => !variant.light));
const lightName = computed(() => findVariant(family, setting.lightVariant)?.name ?? "");
const darkName = computed(() => findVariant(family, setting.darkVariant)?.name ?? "");

function setMode(value: AcceptableValue | AcceptableValue[]): void {
	if (value === "single" || value === "auto") {
		emit("update", { mode: value satisfies ThemeMode });
	}
}

const cardClass =
	"flex items-center gap-3 rounded-md border border-border bg-surface-raised p-2 text-left focus-visible:outline-2 focus-visible:outline-accent";
const markerClass =
	"flex items-center gap-1 rounded px-1.5 py-0.5 text-xs text-muted hover:bg-surface-sunken hover:text-text focus-visible:outline-2 focus-visible:outline-accent aria-pressed:bg-accent aria-pressed:text-on-accent";
</script>

<template>
	<div class="space-y-3">
		<RekaToggleGroupRoot
			type="single"
			:model-value="setting.mode"
			class="inline-flex rounded-md border border-border p-0.5 text-sm"
			@update:model-value="setMode"
		>
			<RekaToggleGroupItem
				v-for="mode in ['single', 'auto'] as const"
				:key="mode"
				:value="mode"
				class="rounded px-3 py-1 text-muted hover:text-text focus-visible:outline-2 focus-visible:outline-accent data-[state=on]:bg-accent data-[state=on]:text-on-accent"
			>
				{{ mode === "single" ? t("pages.settings.modeSingle") : t("pages.settings.modeAuto") }}
			</RekaToggleGroupItem>
		</RekaToggleGroupRoot>

		<RekaRadioGroupRoot
			v-if="setting.mode === 'single'"
			:model-value="setting.variant"
			class="grid grid-cols-[repeat(auto-fill,minmax(13rem,1fr))] gap-2"
			@update:model-value="(value) => typeof value === 'string' && emit('update', { variant: value })"
		>
			<RekaRadioGroupItem
				v-for="variant in family.variants"
				:key="variant.id"
				:value="variant.id"
				:class="[cardClass, 'hover:bg-surface-sunken data-[state=checked]:border-accent data-[state=checked]:bg-accent-surface']"
			>
				<ThemeSwatch :colors="variant.colors" />
				<span class="truncate font-bold">{{ variant.name }}</span>
			</RekaRadioGroupItem>
		</RekaRadioGroupRoot>

		<div
			v-else
			role="group"
			:aria-label="t('pages.settings.variant', { family: family.name })"
			class="grid grid-cols-[repeat(auto-fill,minmax(13rem,1fr))] gap-2"
		>
			<div
				v-for="variant in family.variants"
				:key="variant.id"
				:class="[cardClass, variant.id === setting.lightVariant || variant.id === setting.darkVariant ? 'border-accent' : '']"
			>
				<ThemeSwatch :colors="variant.colors" />
				<span class="min-w-0 flex-1">
					<span class="block truncate font-bold">{{ variant.name }}</span>
					<span class="mt-1 flex gap-1">
						<button
							type="button"
							:class="markerClass"
							:aria-pressed="variant.id === setting.lightVariant"
							:aria-label="t('pages.settings.markLight', { variant: variant.name })"
							@click="emit('update', { lightVariant: variant.id })"
						>
							<i-ph-sun class="size-3.5" />
							{{ t("pages.settings.light") }}
						</button>
						<button
							type="button"
							:class="markerClass"
							:aria-pressed="variant.id === setting.darkVariant"
							:aria-label="t('pages.settings.markDark', { variant: variant.name })"
							@click="emit('update', { darkVariant: variant.id })"
						>
							<i-ph-moon class="size-3.5" />
							{{ t("pages.settings.dark") }}
						</button>
					</span>
				</span>
			</div>
		</div>

		<p v-if="setting.mode === 'auto'" class="text-sm text-muted">
			{{ t("pages.settings.autoHint", { light: lightName, dark: darkName }) }}
			<template v-if="!hasLightSide"> {{ t("pages.settings.sameVariantHint") }}</template>
		</p>
	</div>
</template>
