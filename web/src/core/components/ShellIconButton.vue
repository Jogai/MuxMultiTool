<script setup lang="ts">
/** Icon-only button in the top bar; the label is the tooltip and the accessible name. A disabled one keeps its tooltip, which says why. */
const { label, disabled = false } = defineProps<{ label: string; disabled?: boolean }>();
const emit = defineEmits<{ click: [] }>();
</script>

<template>
	<RekaTooltipRoot>
		<RekaTooltipTrigger as-child>
			<button
				type="button"
				class="flex size-9 items-center justify-center rounded-md text-muted hover:bg-surface-sunken hover:text-text focus-visible:outline-2 focus-visible:outline-accent aria-disabled:cursor-not-allowed aria-disabled:opacity-50 aria-disabled:hover:bg-transparent aria-disabled:hover:text-muted"
				:aria-label="label"
				:aria-disabled="disabled || undefined"
				@click="!disabled && emit('click')"
			>
				<slot />
			</button>
		</RekaTooltipTrigger>
		<RekaTooltipPortal>
			<RekaTooltipContent
				side="bottom"
				:side-offset="4"
				class="rounded-md border border-border bg-surface-raised px-2 py-1 text-sm text-text shadow-md"
			>
				{{ label }}
			</RekaTooltipContent>
		</RekaTooltipPortal>
	</RekaTooltipRoot>
</template>
