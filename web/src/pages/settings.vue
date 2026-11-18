<script setup lang="ts">
import { FAMILIES } from "~/core/themes";

/**
 * Settings, one tab per section: appearance (family first, then variant, auto mode marked on the
 * variant cards), help, about and the software bill of materials. The open tab is the URL hash
 * (`/settings#sbom`), so a link or a reload opens the same tab; no hash is the first tab.
 */
const { t } = useI18n();
const theme = useThemeStore();
const family = computed(() => theme.resolved.family);
const familyId = computed({
	get: () => family.value.id,
	set: (id) => theme.update({ family: id })
});

const route = useRoute();
const router = useRouter();
const tab = computed({
	get: () => settingsTab(route.hash),
	set: (value: SettingsTab) => void router.replace({ hash: value === SETTINGS_TABS[0] ? "" : `#${value}` })
});
</script>

<template>
	<section class="p-6">
		<h2 class="text-xl font-bold">{{ t("nav.settings") }}</h2>
		<p class="mt-2 max-w-prose text-muted">{{ t("pages.settings.description") }}</p>

		<RekaTabsRoot :model-value="tab" class="mt-4" @update:model-value="tab = settingsTab(String($event))">
			<RekaTabsList class="flex flex-wrap gap-1 border-b border-border">
				<RekaTabsTrigger
					v-for="name in SETTINGS_TABS"
					:key="name"
					:value="name"
					class="-mb-px border-b-2 border-transparent px-3 py-2 text-sm text-muted data-[state=active]:border-accent data-[state=active]:text-text focus-visible:outline-2 focus-visible:outline-accent"
				>
					{{ t(`pages.settings.tabs.${name}`) }}
				</RekaTabsTrigger>
			</RekaTabsList>

			<RekaTabsContent value="appearance" class="mt-6 space-y-6">
				<div>
					<h3 class="mb-2 flex items-center gap-2 font-bold">
						<span class="flex size-6 items-center justify-center rounded-full bg-accent text-xs text-on-accent" aria-hidden="true">1</span>
						{{ t("pages.settings.family") }}
					</h3>
					<ThemeFamilyPicker v-model="familyId" :families="FAMILIES" />
				</div>

				<div>
					<h3 class="mb-2 flex items-center gap-2 font-bold">
						<span class="flex size-6 items-center justify-center rounded-full bg-accent text-xs text-on-accent" aria-hidden="true">2</span>
						{{ t("pages.settings.variant", { family: family.name }) }}
					</h3>
					<ThemeVariantPicker :family="family" :setting="theme.setting" @update="(patch) => theme.update(patch)" />
				</div>

				<div v-if="theme.saveError">
					<p class="mb-2 text-sm text-warning">{{ t("pages.settings.saveError") }}</p>
					<ErrorPanel :error="theme.saveError" />
				</div>
			</RekaTabsContent>

			<RekaTabsContent value="help" class="mt-6">
				<SettingsHelp />
			</RekaTabsContent>

			<RekaTabsContent value="about" class="mt-6">
				<h3 class="text-lg font-bold">{{ t("pages.settings.about.title") }}</h3>
				<SettingsAbout class="mt-3" />
			</RekaTabsContent>

			<RekaTabsContent value="sbom" class="mt-6">
				<h3 class="text-lg font-bold">{{ t("pages.settings.sbom.title") }}</h3>
				<SettingsSbom class="mt-3" />
			</RekaTabsContent>
		</RekaTabsRoot>
	</section>
</template>
