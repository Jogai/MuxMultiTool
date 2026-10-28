<script setup lang="ts">
import type { NavName } from "~/core/libs/navigation";

/** Stand-in for a screen whose feature task hasn't landed: what it will do and which task brings it. */
const { name } = defineProps<{ name: NavName }>();
const { t } = useI18n();
</script>

<template>
	<section class="p-6">
		<h2 class="text-xl font-bold">{{ t(`nav.${name}`) }}</h2>
		<p class="mt-2 max-w-prose text-muted">{{ t(`pages.${name}.description`) }}</p>
		<p class="mt-4 text-sm text-muted">{{ t("pages.arrives", { task: t(`pages.${name}.task`) }) }}</p>
		<slot />
	</section>
</template>
