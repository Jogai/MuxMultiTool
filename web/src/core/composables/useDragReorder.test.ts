import { describe, expect, it, vi } from "vitest";
import { ref } from "vue";

import { useDragReorder } from "./useDragReorder";

describe("useDragReorder", () => {
	it("moves the dragged row on drop and the chosen row with moveTo", () => {
		const items = ref(["a", "b", "c"]);
		const reorder = vi.fn();
		const { dragging, moveTo, drop } = useDragReorder(items, reorder);

		drop(1);
		expect(reorder).not.toHaveBeenCalled();

		dragging.value = 2;
		drop(0);
		expect(reorder).toHaveBeenLastCalledWith(["c", "a", "b"], 0);
		expect(dragging.value).toBeUndefined();

		moveTo(0, 1);
		expect(reorder).toHaveBeenLastCalledWith(["b", "a", "c"], 1);
	});
});
