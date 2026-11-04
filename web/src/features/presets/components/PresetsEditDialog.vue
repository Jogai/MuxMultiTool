<script setup lang="ts">
import { create } from "@bufbuild/protobuf";

import { containerExtension, emptyPresetForm, PRESET_CONTAINERS, type PresetFormPart, toStreamsForm } from "../lib/presets";
import { type AppError, toAppError } from "~/core/libs/errors";
import { useRpcClient } from "~/core/libs/rpc";
import { type Preset, PresetContainer, PresetSettingsSchema, PresetsService } from "~/generated/muxmultitool/presets/v1/presets_pb";
import { checkTemplate, renderTemplate, utcDate } from "~/features/batch/lib/template";
import { toStreamRules } from "~/features/streams/lib/streams";

/**
 * Creates a preset, or edits one: its name, container, output name template and rules, with the
 * Streams screen's rules editor. An edit that changes the name renames the preset first, then
 * updates its settings; `saved` carries the preset as the server returned it.
 */
const { preset } = defineProps<{ preset?: Preset }>();
const open = defineModel<boolean>("open", { required: true });
const emit = defineEmits<{ saved: [preset: Preset] }>();
const { t } = useI18n();
const client = useRpcClient(PresetsService);

const name = ref("");
const container = ref<PresetContainer>(PresetContainer.MKV);
const template = ref("{name}");
const part = ref<PresetFormPart>(emptyPresetForm());
const busy = ref(false);
const error = ref<AppError>();

watch(open, (isOpen) => {
	if (!isOpen) {
		return;
	}
	error.value = undefined;
	name.value = preset?.name ?? "";
	container.value = preset?.settings?.container || PresetContainer.MKV;
	template.value = preset?.settings?.template ?? "{name}";
	part.value = preset ? toStreamsForm(preset) : emptyPresetForm();
});

const extension = computed(() => containerExtension(container.value));
const problem = computed(() => checkTemplate(template.value.trim()));
const problemText = computed(() => {
	const found = problem.value;
	if (!found) {
		return undefined;
	}
	return found.kind === "unknown"
		? t("batch.output.problems.template.unknown", { placeholder: found.placeholder })
		: t(`batch.output.problems.template.${found.kind}`);
});
const example = computed(() =>
	problem.value
		? undefined
		: renderTemplate(`${template.value.trim()}.${extension.value}`, {
				name: "Movie",
				ext: "mp4",
				lang: "eng",
				date: utcDate(new Date()),
				n: "01",
				parent: "Movies"
			})
);
const canSave = computed(() => name.value.trim() !== "" && problem.value === undefined && !busy.value);

async function save() {
	if (!canSave.value) {
		return;
	}
	busy.value = true;
	error.value = undefined;
	const settings = create(PresetSettingsSchema, {
		container: container.value,
		template: template.value.trim(),
		rules: toStreamRules(part.value)
	});
	try {
		let saved: Preset | undefined;
		if (preset) {
			if (name.value.trim() !== preset.name) {
				await client.renamePreset({ id: preset.id, name: name.value.trim() });
			}
			saved = (await client.updatePreset({ id: preset.id, settings })).preset;
		} else {
			saved = (await client.createPreset({ name: name.value.trim(), settings })).preset;
		}
		if (saved) {
			emit("saved", saved);
		}
		open.value = false;
	} catch (cause) {
		error.value = toAppError(cause);
	} finally {
		busy.value = false;
	}
}

const field = "rounded-md border border-border bg-surface px-2 py-1 text-sm";
</script>

<template>
	<RekaDialogRoot v-model:open="open">
		<RekaDialogPortal>
			<RekaDialogOverlay class="fixed inset-0 bg-overlay" />
			<RekaDialogContent
				class="fixed top-1/2 left-1/2 flex max-h-[min(44rem,calc(100vh-2rem))] w-[min(48rem,calc(100vw-2rem))] -translate-1/2 flex-col gap-4 overflow-y-auto rounded-lg border border-border bg-surface p-5 shadow-lg"
			>
				<RekaDialogTitle class="text-lg font-bold">
					{{ preset ? t("presets.edit.titleEdit", { name: preset.name }) : t("presets.edit.titleNew") }}
				</RekaDialogTitle>
				<RekaDialogDescription class="sr-only">{{ t("presets.description") }}</RekaDialogDescription>

				<form class="flex flex-col gap-4" @submit.prevent="save">
					<div class="flex flex-wrap gap-4">
						<label class="flex min-w-64 flex-1 flex-col gap-1 text-sm">
							{{ t("presets.edit.name") }}
							<input v-model="name" type="text" :class="field" required :placeholder="t('presets.save.namePlaceholder')" />
						</label>
						<label class="flex flex-col gap-1 text-sm">
							{{ t("presets.edit.container") }}
							<select v-model="container" :class="field">
								<option v-for="option in PRESET_CONTAINERS" :key="option.container" :value="option.container">{{ option.name }}</option>
							</select>
						</label>
					</div>
					<label class="flex flex-col gap-1 text-sm">
						{{ t("presets.edit.template") }}
						<input v-model="template" type="text" :class="field" class="font-mono" :aria-invalid="problem !== undefined" />
						<span class="text-xs text-muted">{{ t("presets.edit.templateHint") }}</span>
						<span v-if="problemText" class="text-xs text-danger" role="alert">{{ problemText }}</span>
						<span v-else-if="example" class="font-mono text-xs text-muted">{{
							t("presets.edit.example", { input: "Movie.mp4", output: example })
						}}</span>
					</label>

					<StreamsRules
						v-model:rules="part.rules"
						v-model:untagged="part.untagged"
						v-model:video-only="part.videoOnly"
						:video-only-switch="false"
					/>

					<ErrorPanel v-if="error" :error="error" />
					<div class="flex justify-end gap-2">
						<RekaDialogClose
							class="rounded-md border border-border px-3 py-1.5 text-sm hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent"
						>
							{{ t("presets.edit.cancel") }}
						</RekaDialogClose>
						<button
							type="submit"
							class="rounded-md bg-accent px-3 py-1.5 text-sm text-on-accent focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-50"
							:disabled="!canSave"
						>
							{{ preset ? t("presets.edit.save") : t("presets.edit.create") }}
						</button>
					</div>
				</form>
			</RekaDialogContent>
		</RekaDialogPortal>
	</RekaDialogRoot>
</template>
