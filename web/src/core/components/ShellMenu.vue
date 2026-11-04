<script setup lang="ts">
import type { InfoDialogKind } from "~/core/components/ShellInfoDialog.vue";

/** The ☰ menu in the top bar: Help and About (TASK-48). */
const emit = defineEmits<{ select: [kind: InfoDialogKind] }>();
const { t } = useI18n();
</script>

<template>
	<RekaDropdownMenuRoot>
		<RekaDropdownMenuTrigger as-child>
			<button
				type="button"
				class="flex size-9 items-center justify-center rounded-md text-muted hover:bg-surface-sunken hover:text-text focus-visible:outline-2 focus-visible:outline-accent data-[state=open]:bg-surface-sunken"
				:aria-label="t('topBar.menu')"
			>
				<i-ph-list />
			</button>
		</RekaDropdownMenuTrigger>
		<RekaDropdownMenuPortal>
			<RekaDropdownMenuContent
				align="end"
				:side-offset="4"
				class="min-w-40 rounded-md border border-border bg-surface-raised p-1 text-sm shadow-md"
			>
				<RekaDropdownMenuItem
					class="cursor-default rounded px-2 py-1.5 outline-none data-[highlighted]:bg-accent data-[highlighted]:text-on-accent"
					@select="emit('select', 'help')"
				>
					{{ t("menu.help") }}
				</RekaDropdownMenuItem>
				<RekaDropdownMenuItem
					class="cursor-default rounded px-2 py-1.5 outline-none data-[highlighted]:bg-accent data-[highlighted]:text-on-accent"
					@select="emit('select', 'about')"
				>
					{{ t("menu.about") }}
				</RekaDropdownMenuItem>
			</RekaDropdownMenuContent>
		</RekaDropdownMenuPortal>
	</RekaDropdownMenuRoot>
</template>
