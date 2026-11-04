import { defineStore } from "pinia";
import { ref } from "vue";

import type { PresetFormPart } from "~/features/presets/lib/presets";
import { emptyForm, type StreamsForm } from "~/features/streams/lib/streams";

/**
 * The Streams screen's form, kept across navigation so the rules a user set survive a detour to the
 * Files screen, and so the Presets screen's "Apply to files" can put a preset's rules, container and
 * template into it before it opens the screen.
 */
export const useStreamsFormStore = defineStore("streamsForm", () => {
	const form = ref<StreamsForm>(emptyForm());

	/** Puts the preset's settings into the form; the files' hand-made ticks are dropped, the output folder stays. */
	function applyPreset(part: PresetFormPart): void {
		form.value = { ...form.value, ...part, overrides: new Map() };
	}

	function reset(): void {
		form.value = emptyForm();
	}

	return { form, applyPreset, reset };
});
