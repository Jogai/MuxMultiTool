<script setup lang="ts">
/**
 * A time field: shows `value` until focused, then holds what is typed. Enter hands the text to the host
 * with a `settle` callback: `settle(true)` ends the edit (the field blurs and shows `value` again),
 * `settle(false)` keeps the draft for a correction. Esc gives up the edit, and so does a blur unless a
 * commit is still settling (the host may be parsing on the server); `end` fires when an edit is given
 * up, so the host can clear what it showed about the draft. `label` is the accessible name; a host that
 * wraps the field in a `<label>` leaves it out.
 */
const {
	value,
	invalid = false,
	dense = false,
	label,
	title
} = defineProps<{ value: string; invalid?: boolean; dense?: boolean; label?: string; title?: string }>();
const emit = defineEmits<{ commit: [text: string, settle: (ok: boolean) => void]; end: [] }>();

const draft = ref<string>();
const settling = ref(false);

function commit(event: Event): void {
	const input = event.target as HTMLInputElement;
	settling.value = true;
	emit("commit", draft.value ?? "", (ok) => {
		settling.value = false;
		if (ok) {
			draft.value = undefined;
			input.blur();
		}
	});
}
function giveUp(): void {
	draft.value = undefined;
	emit("end");
}
function onBlur(): void {
	if (!settling.value && draft.value !== undefined) {
		giveUp();
	}
}
function cancel(event: Event): void {
	giveUp();
	(event.target as HTMLInputElement).blur();
}
</script>

<template>
	<input
		class="w-[14ch] rounded-md border bg-surface-sunken px-1.5 font-mono tabular-nums focus-visible:outline-2 focus-visible:outline-accent"
		:class="[invalid ? 'border-danger' : 'border-border', dense ? 'py-0.5 text-sm' : 'py-1 text-base']"
		:value="draft ?? value"
		:aria-label="label"
		:title="title"
		:aria-invalid="invalid || undefined"
		spellcheck="false"
		@focus="draft = value"
		@input="draft = ($event.target as HTMLInputElement).value"
		@keydown.enter.prevent="commit"
		@keydown.esc.prevent="cancel"
		@blur="onBlur"
	/>
</template>
