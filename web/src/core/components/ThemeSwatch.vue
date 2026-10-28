<script setup lang="ts">
import type { PaletteColors } from "~/core/themes/palette";

/** A variant's background with three of its accents, as on the settings sketch. The colours come from the palette, not from the theme. */
const { colors } = defineProps<{ colors: PaletteColors }>();
</script>

<template>
	<span
		aria-hidden="true"
		class="flex h-8 w-14 shrink-0 items-center justify-center gap-1 rounded border border-border"
		:style="{ backgroundColor: colors.surface }"
	>
		<span class="size-3 rounded-full" :style="{ backgroundColor: colors.accent }" />
		<span class="size-3 rounded-full" :style="{ backgroundColor: colors.success }" />
		<span class="size-3 rounded-full" :style="{ backgroundColor: colors.danger }" />
	</span>
</template>
