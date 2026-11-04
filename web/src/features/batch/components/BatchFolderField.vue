<script setup lang="ts">
import IconFolder from "~icons/lucide/folder";
import IconX from "~icons/lucide/x";

import { useFolderPick } from "../composables/useFolderPick";
import { type AppError, toAppError } from "~/core/libs/errors";

/**
 * A folder chosen through the file picker: its path on show, its id in the model. The Batch screen
 * uses it for the folder to search and the output folder; the latter can be cleared, which puts
 * every output next to its input.
 */
const { label, placeholder, clearable = false } = defineProps<{ label: string; placeholder: string; clearable?: boolean }>();
const id = defineModel<string>("id", { required: true });
const folderLabel = defineModel<string>("name", { required: true });
const { t } = useI18n();
const { pickFolder } = useFolderPick();
const error = ref<AppError>();

async function choose() {
	error.value = undefined;
	try {
		const folder = await pickFolder(id.value || undefined);
		if (folder) {
			id.value = folder.id;
			folderLabel.value = folder.path || folder.name;
		}
	} catch (cause) {
		error.value = toAppError(cause);
	}
}

function clear() {
	id.value = "";
	folderLabel.value = "";
}

const button =
	"rounded-md border border-border px-3 py-1 text-sm hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent";
</script>

<template>
	<div class="flex min-w-64 flex-col gap-1 text-sm">
		<span>{{ label }}</span>
		<div class="flex items-center gap-2">
			<span
				class="flex min-h-8 min-w-0 flex-1 items-center gap-2 rounded-md border border-border bg-surface px-2 py-1 font-mono text-sm"
				:class="{ 'text-muted': !id }"
				:title="folderLabel || undefined"
			>
				<IconFolder class="size-4 shrink-0" aria-hidden="true" />
				<span class="truncate">{{ id ? folderLabel || id : placeholder }}</span>
			</span>
			<button type="button" :class="button" @click="choose">{{ t("batch.folder.choose") }}</button>
			<ShellTooltip v-if="clearable && id" :text="t('batch.folder.clear')">
				<button type="button" :class="button" :aria-label="t('batch.folder.clear')" @click="clear">
					<IconX class="size-4" aria-hidden="true" />
				</button>
			</ShellTooltip>
		</div>
		<ErrorPanel v-if="error" :error="error" />
		<slot />
	</div>
</template>
