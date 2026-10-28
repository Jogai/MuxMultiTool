<script setup lang="ts">
/**
 * The action bar at the bottom of the sketch: "With N selected:" and the screens that start from the
 * selection. Each screen reads the selection store, so the buttons only navigate.
 */
const { count } = defineProps<{ count: number }>();
const emit = defineEmits<{ clear: [] }>();
const { t } = useI18n();

const targets = [
	{ key: "streams", path: "/streams" },
	{ key: "combine", path: "/combine" },
	{ key: "merge", path: "/merge" },
	{ key: "batch", path: "/batch" }
] as const;
</script>

<template>
	<div class="sticky bottom-0 flex flex-wrap items-center gap-2 border-t border-border bg-surface-raised px-4 py-2 text-sm">
		<span class="mr-2" :class="count === 0 ? 'text-muted' : 'font-bold'">
			{{ count === 0 ? t("files.actions.none") : t("files.actions.with", { n: count }) }}
		</span>
		<RouterLink
			v-for="target in targets"
			:key="target.key"
			:to="target.path"
			class="rounded-md border border-border px-3 py-1 hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent aria-disabled:pointer-events-none aria-disabled:opacity-50"
			:aria-disabled="count === 0 || undefined"
			:tabindex="count === 0 ? -1 : undefined"
		>
			{{ t(`files.actions.${target.key}`) }}
		</RouterLink>
		<button
			v-if="count > 0"
			type="button"
			class="ml-auto rounded-md px-3 py-1 text-muted hover:bg-surface-sunken hover:text-text focus-visible:outline-2 focus-visible:outline-accent"
			@click="emit('clear')"
		>
			{{ t("files.actions.clear") }}
		</button>
	</div>
</template>
