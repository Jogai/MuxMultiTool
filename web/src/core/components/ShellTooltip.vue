<script setup lang="ts">
/**
 * A hint on any element, in place of a `title`: it shows on hover and on keyboard focus, and assistive
 * technology reads it as the element's description. Without text the element is rendered on its own.
 * The slot must hold exactly one element; a `disabled` button fires no events, so a hint on one that
 * says why it is off needs `aria-disabled` instead.
 */
const { text, side = "bottom" } = defineProps<{ text?: string; side?: "top" | "bottom" | "left" | "right" }>();
</script>

<template>
	<RekaTooltipRoot v-if="text">
		<RekaTooltipTrigger as-child>
			<slot />
		</RekaTooltipTrigger>
		<RekaTooltipPortal>
			<RekaTooltipContent
				:side="side"
				:side-offset="4"
				class="max-w-xs rounded-md border border-border bg-surface-raised px-2 py-1 text-sm text-text shadow-md"
			>
				{{ text }}
			</RekaTooltipContent>
		</RekaTooltipPortal>
	</RekaTooltipRoot>
	<slot v-else />
</template>
