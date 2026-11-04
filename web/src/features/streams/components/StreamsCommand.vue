<script setup lang="ts">
/** The exact ffmpeg command the job would run, live, with a Copy button. Shared by the Streams and Combine screens. */
const { command, title } = defineProps<{ command: string; title: string }>();
const { t } = useI18n();
const clipboard = useClipboard();

/** Copies the command, for the `c` shortcut. */
function copy() {
	if (command) {
		void clipboard.copy(command);
	}
}
defineExpose({ copy });
</script>

<template>
	<section :aria-label="title">
		<div class="mb-1 flex items-center gap-3">
			<h4 class="text-sm font-bold">{{ title }}</h4>
			<button
				type="button"
				class="ml-auto rounded-md border border-border px-3 py-0.5 text-sm hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-50"
				:disabled="!command"
				@click="copy"
			>
				{{ clipboard.copied.value ? t("streams.command.copied") : t("streams.command.copy") }}
			</button>
		</div>
		<pre v-if="command" class="overflow-x-auto rounded-md border border-border bg-surface-sunken p-3 text-xs whitespace-pre-wrap">{{
			command
		}}</pre>
		<p v-else class="text-sm text-muted">{{ t("streams.command.none") }}</p>
	</section>
</template>
