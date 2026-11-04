<script setup lang="ts">
import type { FlattenedItem } from "reka-ui";

import type { FolderNode } from "../lib/folderTree";

/**
 * One row of the folder tree: a folder with its chevron and icon, or the note under a folder whose
 * subfolders are not there (yet). The row is the tree item, so it takes the focus; Enter and a click
 * on it go to the folder, Left and Right (or the chevron) fold and unfold it.
 */
const { item, currentId } = defineProps<{ item: FlattenedItem<FolderNode>; currentId: string }>();
const emit = defineEmits<{ navigate: [id: string] }>();
const { t } = useI18n();

const node = computed(() => item.value);
const isCurrent = computed(() => node.value.kind === "folder" && node.value.id === currentId);
const indent = computed(() => `${(item.level - 1) * 0.75}rem`);
const noteText = computed(() => {
	const current = node.value;
	if (current.kind !== "note") {
		return "";
	}
	switch (current.note) {
		case "loading":
			return "…";
		case "error":
			return t("files.tree.error", { name: current.parent.name });
		default:
			return t("files.tree.empty");
	}
});

interface TreeEvent {
	detail: { originalEvent: Event };
	preventDefault: () => void;
}
function onSelect(event: TreeEvent): void {
	event.preventDefault();
	if (node.value.kind === "folder") {
		emit("navigate", node.value.id);
	}
}
// A click on the row goes to the folder, and the breadcrumbs then open it; only the chevron and the keyboard fold one
function onToggle(event: TreeEvent): void {
	if (event.detail.originalEvent.type === "click") {
		event.preventDefault();
	}
}
</script>

<template>
	<RekaTreeItem
		v-slot="{ isExpanded, handleToggle }"
		v-bind="item.bind"
		:disabled="node.kind === 'note'"
		as-child
		@select="onSelect"
		@toggle="onToggle"
	>
		<li
			class="rounded-md outline-none focus-visible:outline-2 focus-visible:outline-accent"
			:aria-current="isCurrent ? 'location' : undefined"
		>
			<div
				v-if="node.kind === 'folder'"
				class="flex items-center gap-0.5 rounded-md pr-1"
				:class="isCurrent ? 'bg-accent-surface text-accent' : 'hover:bg-surface-sunken'"
				:style="{ paddingLeft: indent }"
				:title="node.entry.path || node.entry.name"
			>
				<span
					class="flex size-6 shrink-0 items-center justify-center rounded text-muted hover:text-text"
					aria-hidden="true"
					@click.stop="handleToggle"
				>
					<i-ph-caret-right class="size-3.5 transition-transform" :class="{ 'rotate-90': isExpanded }" />
				</span>
				<i-ph-folder-open v-if="isExpanded" class="size-4 shrink-0" :class="isCurrent ? 'text-accent' : 'text-muted'" />
				<i-ph-folder v-else class="size-4 shrink-0" :class="isCurrent ? 'text-accent' : 'text-muted'" />
				<span class="truncate py-1">{{ node.entry.name }}</span>
			</div>
			<div
				v-else
				class="py-1 text-xs"
				:class="node.note === 'error' ? 'text-danger' : 'text-muted'"
				:style="{ paddingLeft: `${(item.level - 1) * 0.75 + 1.5}rem` }"
				:title="node.message"
			>
				{{ noteText }}
			</div>
		</li>
	</RekaTreeItem>
</template>
