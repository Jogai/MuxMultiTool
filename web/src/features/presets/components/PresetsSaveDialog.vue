<script setup lang="ts">
import { toPresetSettings, unsavableReason } from "../lib/presets";
import { type AppError, toAppError } from "~/core/libs/errors";
import { useRpcClient } from "~/core/libs/rpc";
import { type Preset, PresetsService } from "~/generated/muxmultitool/presets/v1/presets_pb";
import type { StreamsForm } from "~/features/streams/lib/streams";

/**
 * "Save as preset…" on the Streams screen: names the screen's rules, container and output name and
 * saves them as a preset. What a preset cannot hold (extraction, video only, an audio-only
 * container) is said instead of a form.
 */
const { form } = defineProps<{ form: StreamsForm }>();
const open = defineModel<boolean>("open", { required: true });
const { t } = useI18n();
const client = useRpcClient(PresetsService);

const name = ref("");
const busy = ref(false);
const error = ref<AppError>();
const saved = ref<Preset>();

watch(open, (isOpen) => {
	if (isOpen) {
		name.value = "";
		error.value = undefined;
		saved.value = undefined;
	}
});

const reason = computed(() => unsavableReason(form));
const canSave = computed(() => reason.value === undefined && name.value.trim() !== "" && !busy.value);

async function save() {
	const settings = toPresetSettings(form);
	if (!canSave.value || !settings) {
		return;
	}
	busy.value = true;
	error.value = undefined;
	try {
		saved.value = (await client.createPreset({ name: name.value.trim(), settings })).preset;
	} catch (cause) {
		error.value = toAppError(cause);
	} finally {
		busy.value = false;
	}
}

const field = "rounded-md border border-border bg-surface px-2 py-1 text-sm";
const secondary =
	"rounded-md border border-border px-3 py-1.5 text-sm hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent";
</script>

<template>
	<RekaDialogRoot v-model:open="open">
		<RekaDialogPortal>
			<RekaDialogOverlay class="fixed inset-0 bg-overlay" />
			<RekaDialogContent
				class="fixed top-1/2 left-1/2 flex w-[min(32rem,calc(100vw-2rem))] -translate-1/2 flex-col gap-4 rounded-lg border border-border bg-surface p-5 shadow-lg"
			>
				<RekaDialogTitle class="text-lg font-bold">{{ t("presets.save.title") }}</RekaDialogTitle>
				<RekaDialogDescription class="text-sm text-muted">{{ t("presets.save.body") }}</RekaDialogDescription>

				<p v-if="saved" class="text-sm" role="status">
					{{ t("presets.save.saved", { name: saved.name }) }}
					<RouterLink to="/presets" class="text-accent underline">{{ t("presets.save.open") }}</RouterLink>
				</p>
				<p v-else-if="reason" class="text-sm text-warning" role="status">{{ t(`presets.save.reason.${reason}`) }}</p>
				<form v-else class="flex flex-col gap-3" @submit.prevent="save">
					<label class="flex flex-col gap-1 text-sm">
						{{ t("presets.save.name") }}
						<input v-model="name" type="text" :class="field" required :placeholder="t('presets.save.namePlaceholder')" />
					</label>
					<ErrorPanel v-if="error" :error="error" />
					<div class="flex justify-end gap-2">
						<RekaDialogClose :class="secondary">{{ t("presets.save.cancel") }}</RekaDialogClose>
						<button
							type="submit"
							class="rounded-md bg-accent px-3 py-1.5 text-sm text-on-accent focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-50"
							:disabled="!canSave"
						>
							{{ t("presets.save.create") }}
						</button>
					</div>
				</form>
				<div v-if="saved || reason" class="flex justify-end">
					<RekaDialogClose :class="secondary">{{ t("presets.save.cancel") }}</RekaDialogClose>
				</div>
			</RekaDialogContent>
		</RekaDialogPortal>
	</RekaDialogRoot>
</template>
