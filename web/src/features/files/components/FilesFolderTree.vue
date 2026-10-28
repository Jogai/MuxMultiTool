<script setup lang="ts">
import type { AppError } from "~/core/libs/errors";
import type { Entry } from "~/generated/muxmultitool/files/v1/files_pb";

import { childrenOf, type FolderNode, folderNode, type LoadedFolder } from "../lib/folderTree";

/**
 * The folder tree of the sketch: the media roots at the top, each expandable down to the current
 * folder. Reka's tree gives the rows the roving focus, the arrow keys and the ARIA state; the
 * subfolders of a folder are loaded the first time it is expanded.
 */
const { roots, currentId, expandIds, load, error } = defineProps<{
	roots: Entry[] | undefined;
	currentId: string;
	expandIds: ReadonlySet<string>;
	load: (id: string) => Promise<Entry[]>;
	error?: AppError;
}>();
const emit = defineEmits<{ navigate: [id: string] }>();
const { t } = useI18n();

const items = computed(() => roots?.map(folderNode) ?? []);
const loaded = reactive(new Map<string, LoadedFolder>());
const pending = new Set<string>();
const expanded = ref<string[]>([]);
const selected = computed(() => ({ id: currentId }));
const getKey = (node: FolderNode) => node.id;
const getChildren = (node: FolderNode) => childrenOf(node, loaded);

async function loadChildren(id: string): Promise<void> {
	if (loaded.has(id) || pending.has(id)) {
		return;
	}
	pending.add(id);
	try {
		loaded.set(id, { children: (await load(id)).map(folderNode) });
	} catch (cause) {
		loaded.set(id, { error: toAppError(cause) });
	} finally {
		pending.delete(id);
	}
}

// A folder on the way to the current one opens by itself, so the tree follows the breadcrumbs
watch(
	() => expandIds,
	(ids) => {
		const missing = [...ids].filter((id) => !expanded.value.includes(id));
		if (missing.length > 0) {
			expanded.value = [...expanded.value, ...missing];
		}
	},
	{ immediate: true }
);
// Expanding a folder loads its subfolders; expanding one that failed tries again
watch(
	expanded,
	(ids, previous) => {
		for (const id of ids) {
			if (!previous?.includes(id)) {
				if (loaded.get(id)?.error) {
					loaded.delete(id);
				}
				void loadChildren(id);
			}
		}
	},
	{ immediate: true }
);
</script>

<template>
	<nav :aria-label="t('files.tree.label')" class="min-w-0 text-sm">
		<h3 class="px-2 pb-1 text-xs text-muted uppercase">{{ t("files.tree.label") }}</h3>
		<ErrorPanel v-if="error" :error="error" class="mx-1" />
		<p v-else-if="roots === undefined" class="px-2 py-1 text-xs text-muted">…</p>
		<p v-else-if="roots.length === 0" class="px-2 py-1 text-xs text-muted">{{ t("files.tree.noRoots") }}</p>
		<RekaTreeRoot
			v-else
			v-slot="{ flattenItems }"
			v-model:expanded="expanded"
			:items="items"
			:get-key="getKey"
			:get-children="getChildren"
			:model-value="selected"
			as-child
		>
			<ul :aria-label="t('files.roots')">
				<FilesFolderNode
					v-for="item in flattenItems"
					:key="item._id"
					:item="item"
					:current-id="currentId"
					@navigate="emit('navigate', $event)"
				/>
			</ul>
		</RekaTreeRoot>
	</nav>
</template>
