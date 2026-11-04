import { type MaybeRefOrGetter, ref, toValue } from "vue";

import { move } from "~/core/libs/collections";

/**
 * Reordering the rows of a list by native drag and drop, with `moveTo` for the arrow buttons (the
 * keyboard path). A row's handle sets `dragging` on dragstart and clears it on dragend; the row takes
 * `drop(index)`. `reorder` gets the new list and the index the item landed on.
 */
export function useDragReorder<T>(items: MaybeRefOrGetter<readonly T[]>, reorder: (next: T[], to: number) => void) {
	const dragging = ref<number>();

	function moveTo(from: number, to: number): void {
		reorder(move(toValue(items), from, to), to);
	}

	function drop(to: number): void {
		if (dragging.value !== undefined) {
			moveTo(dragging.value, to);
		}
		dragging.value = undefined;
	}

	return { dragging, moveTo, drop };
}
