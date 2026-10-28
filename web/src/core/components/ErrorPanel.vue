<script setup lang="ts">
import type { AppError } from "~/core/libs/errors";

/** Uniform error display: a title from the Connect code, the plain-language message, and the raw ffmpeg output folded away. */
const { error } = defineProps<{ error: AppError }>();
const { t, te } = useI18n();
const title = computed(() => (te(`errors.codes.${error.codeName}`) ? t(`errors.codes.${error.codeName}`) : t("errors.codes.unknown")));
const rawOpen = ref(false);
</script>

<template>
	<div role="alert" class="rounded-md border border-danger bg-danger-surface p-3 text-sm">
		<p class="font-bold text-danger">{{ title }}</p>
		<p class="mt-1">{{ error.message }}</p>
		<RekaCollapsibleRoot v-if="error.raw" v-model:open="rawOpen" class="mt-2">
			<RekaCollapsibleTrigger class="text-xs text-muted underline">
				{{ rawOpen ? t("errors.hideRaw") : t("errors.showRaw") }}
			</RekaCollapsibleTrigger>
			<RekaCollapsibleContent>
				<pre
					class="mt-2 max-h-64 overflow-auto rounded bg-surface-sunken p-2 font-mono text-xs whitespace-pre-wrap"
					:aria-label="t('errors.rawOutput')"
					>{{ error.raw }}</pre>
			</RekaCollapsibleContent>
		</RekaCollapsibleRoot>
	</div>
</template>
