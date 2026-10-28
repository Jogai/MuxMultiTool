<script setup lang="ts">
/**
 * The container file browser, opened by `pickFiles()` through the file-picker store.
 * The browser over the files service comes with the Files screen (TASK-18); until then it can only cancel.
 */
const { t } = useI18n();
const picker = useFilePickerStore();
</script>

<template>
	<RekaDialogRoot :open="picker.isOpen" @update:open="(value) => !value && picker.cancel()">
		<RekaDialogPortal>
			<RekaDialogOverlay class="fixed inset-0 bg-overlay" />
			<RekaDialogContent
				class="fixed top-1/2 left-1/2 w-[min(40rem,calc(100vw-2rem))] -translate-1/2 rounded-lg border border-border bg-surface p-5 shadow-lg"
			>
				<RekaDialogTitle class="text-lg font-bold">
					{{ picker.pending?.options.directory ? t("filePicker.titleDirectory") : t("filePicker.title") }}
				</RekaDialogTitle>
				<RekaDialogDescription class="mt-2 text-sm text-muted">{{ t("filePicker.placeholder") }}</RekaDialogDescription>
				<p v-if="picker.pending?.options.startIn" class="mt-2 font-mono text-sm">
					{{ t("filePicker.startIn", { path: picker.pending.options.startIn }) }}
				</p>
				<div class="mt-5 flex justify-end">
					<RekaDialogClose
						class="rounded-md border border-border px-3 py-1.5 text-sm hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent"
					>
						{{ t("filePicker.cancel") }}
					</RekaDialogClose>
				</div>
			</RekaDialogContent>
		</RekaDialogPortal>
	</RekaDialogRoot>
</template>
