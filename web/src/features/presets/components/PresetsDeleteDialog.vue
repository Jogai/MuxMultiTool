<script setup lang="ts">
import { type AppError, toAppError } from "~/core/libs/errors";
import { useRpcClient } from "~/core/libs/rpc";
import { type Preset, PresetsService } from "~/generated/muxmultitool/presets/v1/presets_pb";

/** Asks before a preset is deleted: the model is the preset in question, `undefined` when closed. */
const preset = defineModel<Preset>();
const emit = defineEmits<{ deleted: [id: string] }>();
const { t } = useI18n();
const client = useRpcClient(PresetsService);
const busy = ref(false);
const error = ref<AppError>();

function close() {
	preset.value = undefined;
	error.value = undefined;
}

async function confirm() {
	if (!preset.value) {
		return;
	}
	busy.value = true;
	error.value = undefined;
	try {
		await client.deletePreset({ id: preset.value.id });
		emit("deleted", preset.value.id);
		close();
	} catch (cause) {
		error.value = toAppError(cause);
	} finally {
		busy.value = false;
	}
}
</script>

<template>
	<RekaAlertDialogRoot :open="preset !== undefined" @update:open="(value) => !value && close()">
		<RekaAlertDialogPortal>
			<RekaAlertDialogOverlay class="fixed inset-0 bg-overlay" />
			<RekaAlertDialogContent
				class="fixed top-1/2 left-1/2 w-[min(28rem,calc(100vw-2rem))] -translate-1/2 rounded-lg border border-border bg-surface p-5 shadow-lg"
			>
				<RekaAlertDialogTitle class="text-lg font-bold">{{ t("presets.delete.title", { name: preset?.name ?? "" }) }}</RekaAlertDialogTitle>
				<RekaAlertDialogDescription class="mt-2 text-sm text-muted">{{ t("presets.delete.body") }}</RekaAlertDialogDescription>
				<ErrorPanel v-if="error" :error="error" class="mt-3" />
				<div class="mt-5 flex justify-end gap-2">
					<RekaAlertDialogCancel
						class="rounded-md border border-border px-3 py-1.5 text-sm hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent"
					>
						{{ t("presets.delete.cancel") }}
					</RekaAlertDialogCancel>
					<button
						type="button"
						class="rounded-md bg-danger px-3 py-1.5 text-sm text-on-accent focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-60"
						:disabled="busy"
						@click="confirm"
					>
						{{ t("presets.delete.confirm") }}
					</button>
				</div>
			</RekaAlertDialogContent>
		</RekaAlertDialogPortal>
	</RekaAlertDialogRoot>
</template>
