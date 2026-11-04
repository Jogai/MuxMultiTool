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

const error = ref<AppError>();

async function commit(draft: string, settle: (ok: boolean) => void): Promise<void> {
	const text = draft.trim();
	if (text === "") {
		settle(false);
		return;
	}
	try {
		const response = await client.parseTime({ text, fileId });
		error.value = undefined;
		emit("commit", response.seconds);
		settle(true);
	} catch (cause) {
		error.value = toAppError(cause);
		settle(false);
	}
}
</script>

<template>
	<span class="inline-flex flex-col">
		<ShellTimecodeInput
			:value="formatTimecode(seconds)"
			:invalid="error !== undefined || invalid"
			dense
			:label="label"
			:title="t('cut.segments.typeHint')"
			@commit="commit"
			@end="error = undefined"
		/>
		<span v-if="error" class="text-xs text-danger">{{ error.message }}</span>
	</span>
</template>
