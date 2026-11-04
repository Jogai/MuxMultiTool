<script setup lang="ts">
import { type AppError, toAppError } from "~/core/libs/errors";
import { formatTimecode } from "~/core/libs/format";
import { useRpcClient } from "~/core/libs/rpc";
import { CutService } from "~/generated/muxmultitool/cut/v1/cut_pb";

/**
 * A typed cut time: shows the time as a timecode until focused, then holds what is typed. Enter sends
 * the text to `CutService.ParseTime`, which accepts every form the backend knows (`12:34`, `1h2m3.5s`,
 * `754.5`, frames as `0:12:34:12`) and refuses a time past the end of the file; the parsed seconds are
 * emitted and the host decides whether the pair of times is valid. Esc gives up the edit.
 */
const {
	seconds,
	fileId,
	label,
	invalid = false
} = defineProps<{
	seconds: number;
	fileId: string;
	label: string;
	/** Marks the field (the host found the pair of times wrong). */
	invalid?: boolean;
}>();
const emit = defineEmits<{ commit: [seconds: number] }>();
const { t } = useI18n();
const client = useRpcClient(CutService);

const draft = ref<string>();
const parsing = ref(false);
const error = ref<AppError>();
const shown = computed(() => draft.value ?? formatTimecode(seconds));

async function commit(event: Event): Promise<void> {
	const input = event.target as HTMLInputElement;
	const text = (draft.value ?? "").trim();
	if (text === "") {
		return;
	}
	parsing.value = true;
	try {
		const response = await client.parseTime({ text, fileId });
		error.value = undefined;
		draft.value = undefined;
		emit("commit", response.seconds);
		input.blur();
	} catch (cause) {
		error.value = toAppError(cause);
	} finally {
		parsing.value = false;
	}
}
function cancel(event: Event): void {
	draft.value = undefined;
	error.value = undefined;
	(event.target as HTMLInputElement).blur();
}
function onBlur(): void {
	// A blur while parsing comes from the commit itself
	if (!parsing.value) {
		draft.value = undefined;
		error.value = undefined;
	}
}
</script>

<template>
	<span class="inline-flex flex-col">
		<input
			class="w-[14ch] rounded-md border bg-surface-sunken px-1.5 py-0.5 font-mono text-sm tabular-nums focus-visible:outline-2 focus-visible:outline-accent"
			:class="error || invalid ? 'border-danger' : 'border-border'"
			:value="shown"
			:aria-label="label"
			:aria-invalid="error || invalid ? true : undefined"
			:title="t('cut.segments.typeHint')"
			spellcheck="false"
			@focus="draft = formatTimecode(seconds)"
			@input="draft = ($event.target as HTMLInputElement).value"
			@keydown.enter.prevent="commit"
			@keydown.esc.prevent="cancel"
			@blur="onBlur"
		/>
		<span v-if="error" class="text-xs text-danger">{{ error.message }}</span>
	</span>
</template>
