<script setup lang="ts">
const { t } = useI18n();
const { state, version, error, refresh } = useSystemStatus();
</script>

<template>
	<section class="p-4">
		<h2 class="font-bold">{{ t("backend.title") }}</h2>
		<p>
			<template v-if="state === 'loading'">{{ t("backend.loading") }}</template>
			<template v-else-if="state === 'ok'">{{ t("backend.ok", { version }) }}</template>
			<template v-else>{{ t("backend.error", { error }) }}</template>
		</p>
		<button type="button" class="underline" @click="refresh">{{ t("backend.refresh") }}</button>
	</section>
</template>
