<script setup lang="ts">
import { FAMILIES } from "~/core/themes";

/**
 * Settings: the appearance section from the sketch (family first, then variant, auto mode marked on the
 * variant cards), then Help, About and the software bill of materials, with a short table of contents
 * so the long bill at the end is one click away.
 */
const { t } = useI18n();
const theme = useThemeStore();
const family = computed(() => theme.resolved.family);
const familyId = computed({
	get: () => family.value.id,
	set: (id) => theme.update({ family: id })
});
const SECTIONS = ["appearance", "help", "about", "sbom"] as const;
</script>

<template>
	<section class="p-6">
		<h2 class="text-xl font-bold">{{ t("nav.settings") }}</h2>
		<p class="mt-2 max-w-prose text-muted">{{ t("pages.settings.description") }}</p>
		<nav :aria-label="t('pages.settings.contents')" class="mt-3 flex flex-wrap gap-x-4 gap-y-1 text-sm">
			<a
				v-for="section in SECTIONS"
				:key="section"
				:href="`#${section}`"
				class="text-accent underline focus-visible:outline-2 focus-visible:outline-accent"
			>
				{{ t(section === "appearance" ? "pages.settings.appearance" : `pages.settings.${section}.title`) }}
			</a>
		</nav>

		<section id="appearance" class="mt-8 scroll-mt-4 space-y-6">
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

			<div v-if="theme.saveError">
				<p class="mb-2 text-sm text-warning">{{ t("pages.settings.saveError") }}</p>
				<ErrorPanel :error="theme.saveError" />
			</div>
		</section>

		<section id="help" class="mt-10 scroll-mt-4">
			<h3 class="text-lg font-bold">{{ t("pages.settings.help.title") }}</h3>
			<SettingsHelp class="mt-3" />
		</section>

		<section id="about" class="mt-10 scroll-mt-4">
			<h3 class="text-lg font-bold">{{ t("pages.settings.about.title") }}</h3>
			<SettingsAbout class="mt-3" />
		</section>

		<section id="sbom" class="mt-10 scroll-mt-4">
			<h3 class="text-lg font-bold">{{ t("pages.settings.sbom.title") }}</h3>
			<SettingsSbom class="mt-3" />
		</section>
	</section>
</template>
