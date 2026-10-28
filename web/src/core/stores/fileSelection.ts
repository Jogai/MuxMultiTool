import { defineStore } from "pinia";
import { computed, shallowRef } from "vue";

import type { Entry } from "~/generated/muxmultitool/files/v1/files_pb";

/**
 * The files chosen on the Files screen, in the order they were chosen. This selection is the input
 * for every other screen (gui/README.md): streams, combine, cut, merge and batch all start from it
 * instead of opening files again. Keyed by the opaque file id.
 */
export const useFileSelectionStore = defineStore("fileSelection", () => {
	const byId = shallowRef(new Map<string, Entry>());

	const files = computed(() => [...byId.value.values()]);
	const ids = computed(() => [...byId.value.keys()]);
	const count = computed(() => byId.value.size);
	const isEmpty = computed(() => byId.value.size === 0);

	function has(id: string): boolean {
		return byId.value.has(id);
	}

	/** Adds files to the selection; an entry that is already in it keeps its place. */
	function add(entries: readonly Entry[]): void {
		const next = new Map(byId.value);
		for (const entry of entries) {
			if (!next.has(entry.id)) {
				next.set(entry.id, entry);
			}
		}
		byId.value = next;
	}

	function remove(ids: readonly string[]): void {
		const next = new Map(byId.value);
		for (const id of ids) {
			next.delete(id);
		}
		byId.value = next;
	}

	function toggle(entry: Entry): void {
		if (byId.value.has(entry.id)) {
			remove([entry.id]);
		} else {
			add([entry]);
		}
	}

	/** Replaces the whole selection. */
	function set(entries: readonly Entry[]): void {
		byId.value = new Map(entries.map((entry) => [entry.id, entry]));
	}

	function clear(): void {
		set([]);
	}

	return { files, ids, count, isEmpty, has, add, remove, toggle, set, clear };
});
