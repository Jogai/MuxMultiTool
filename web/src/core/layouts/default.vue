<script setup lang="ts">
const { t } = useI18n();
</script>

<template>
	<div class="min-h-screen bg-surface text-text">
		<main>
			<h1 class="sr-only">{{ t("app.name") }}</h1>
			<RouterView />
		</main>
	</div>
</template>
