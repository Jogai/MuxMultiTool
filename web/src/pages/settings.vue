<script setup lang="ts">
/** Settings placeholder with the backend check, so an unreachable backend has somewhere to show its error. */
const { t } = useI18n();
const system = useSystemStore();
</script>

<template>
	<PlaceholderPage name="settings">
		<section class="mt-6 max-w-prose">
			<h3 class="font-bold">{{ t("pages.settings.backend") }}</h3>
			<p v-if="system.state === 'loading'" class="mt-1 text-sm text-muted">{{ t("pages.settings.backendLoading") }}</p>
			<p v-else-if="system.state === 'ok'" class="mt-1 text-sm text-success">
				{{ t("pages.settings.backendOk", { version: system.version }) }}
			</p>
			<ErrorPanel v-else-if="system.error" :error="system.error" class="mt-2" />
			<button type="button" class="mt-3 text-sm underline" @click="system.refresh()">{{ t("pages.settings.refresh") }}</button>
		</section>
	</PlaceholderPage>
</template>
