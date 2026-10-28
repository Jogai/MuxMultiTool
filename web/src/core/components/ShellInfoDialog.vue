<script lang="ts">
/** Which placeholder dialog is open: Help, or About until TASK-48 brings the real one. */
export type InfoDialogKind = "help" | "about";
</script>

<script setup lang="ts">
const kind = defineModel<InfoDialogKind>();
const { t } = useI18n();
const open = computed(() => kind.value !== undefined);
</script>

<template>
	<RekaDialogRoot :open="open" @update:open="(value) => !value && (kind = undefined)">
		<RekaDialogPortal>
			<RekaDialogOverlay class="fixed inset-0 bg-overlay" />
			<RekaDialogContent
				class="fixed top-1/2 left-1/2 w-[min(28rem,calc(100vw-2rem))] -translate-1/2 rounded-lg border border-border bg-surface p-5 shadow-lg"
			>
				<RekaDialogTitle class="text-lg font-bold">{{ t(`dialogs.${kind}.title`) }}</RekaDialogTitle>
				<RekaDialogDescription class="mt-2 text-sm text-muted">{{ t(`dialogs.${kind}.body`) }}</RekaDialogDescription>
				<p v-if="kind === 'about'" class="mt-3 text-sm">
					{{ t("app.tagline") }}
					<br />
					{{ t("dialogs.about.license") }}
				</p>
				<div class="mt-5 flex justify-end">
					<RekaDialogClose
						class="rounded-md bg-accent px-3 py-1.5 text-sm text-on-accent focus-visible:outline-2 focus-visible:outline-accent"
					>
						{{ t("dialogs.close") }}
					</RekaDialogClose>
				</div>
			</RekaDialogContent>
		</RekaDialogPortal>
	</RekaDialogRoot>
</template>
