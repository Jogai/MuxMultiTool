<script setup lang="ts">
import { FAMILIES } from "~/core/themes";

/**
 * Settings: the appearance section from the sketch (family first, then variant, auto mode marked on the
 * variant cards), and the backend check so an unreachable backend has somewhere to show its error.
 */
const { t } = useI18n();
const system = useSystemStore();
const theme = useThemeStore();
const family = computed(() => theme.resolved.family);
const familyId = computed({
	get: () => family.value.id,
	set: (id) => theme.update({ family: id })
});
</script>

<template>
	<section class="p-6">
		<h2 class="text-xl font-bold">{{ t("nav.settings") }}</h2>
		<p class="mt-2 max-w-prose text-muted">{{ t("pages.settings.description") }}</p>

		<section class="mt-6 space-y-6">
			<h3 class="text-lg font-bold">{{ t("pages.settings.appearance") }}</h3>

			<div>
				<h4 class="mb-2 flex items-center gap-2 font-bold">
					<span class="flex size-6 items-center justify-center rounded-full bg-accent text-xs text-on-accent" aria-hidden="true">1</span>
					{{ t("pages.settings.family") }}
				</h4>
				<ThemeFamilyPicker v-model="familyId" :families="FAMILIES" />
			</div>

			<div>
				<h4 class="mb-2 flex items-center gap-2 font-bold">
					<span class="flex size-6 items-center justify-center rounded-full bg-accent text-xs text-on-accent" aria-hidden="true">2</span>
					{{ t("pages.settings.variant", { family: family.name }) }}
				</h4>
				<ThemeVariantPicker :family="family" :setting="theme.setting" @update="(patch) => theme.update(patch)" />
			</div>

			<p class="text-sm text-muted">{{ t("pages.settings.later") }}</p>
			<div v-if="theme.saveError">
				<p class="mb-2 text-sm text-warning">{{ t("pages.settings.saveError") }}</p>
				<ErrorPanel :error="theme.saveError" />
			</div>
		</section>

		<section class="mt-8 max-w-prose">
			<h3 class="font-bold">{{ t("pages.settings.backend") }}</h3>
			<p v-if="system.state === 'loading'" class="mt-1 text-sm text-muted">{{ t("pages.settings.backendLoading") }}</p>
			<p v-else-if="system.state === 'ok'" class="mt-1 text-sm text-success">
				{{ t("pages.settings.backendOk", { version: system.version }) }}
			</p>
			<ErrorPanel v-else-if="system.error" :error="system.error" class="mt-2" />
			<button type="button" class="mt-3 text-sm underline" @click="system.refresh()">{{ t("pages.settings.refresh") }}</button>
		</section>
	</section>
</template>
