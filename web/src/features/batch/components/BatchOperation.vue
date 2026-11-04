<script setup lang="ts">
import type { OperationSource } from "../lib/batch";
import { streamTypeName } from "~/core/libs/reportFormat";
import type { Preset } from "~/generated/muxmultitool/presets/v1/presets_pb";
import { StreamType } from "~/generated/muxmultitool/files/v1/files_pb";
import { batchTemplateOf, containerName, rulesSummary } from "~/features/presets/lib/presets";
import type { RuleDraft } from "~/features/streams/lib/streams";

/**
 * Step 2 of the sketch: the operation every matching file gets. Either the screen's own stream rules
 * (the Streams screen's editor; no rules is a remux) or a saved preset, which the server turns into
 * the same kind of operation.
 */
const { presets } = defineProps<{ presets: readonly Preset[] }>();
const source = defineModel<OperationSource>("source", { required: true });
const presetId = defineModel<string>("presetId", { required: true });
const rules = defineModel<readonly RuleDraft[]>("rules", { required: true });
const untagged = defineModel<"keep" | "drop">("untagged", { required: true });
const videoOnly = defineModel<boolean>("videoOnly", { required: true });
const { t } = useI18n();

const preset = computed(() => presets.find((entry) => entry.id === presetId.value));
const typeName = (type: StreamType) => (type === StreamType.UNSPECIFIED ? t("streams.rules.anyType") : streamTypeName(type));
const presetRules = computed(() => (preset.value ? rulesSummary(preset.value.settings, typeName) : ""));

const segment =
	"rounded px-3 py-0.5 text-muted hover:text-text focus-visible:outline-2 focus-visible:outline-accent data-[state=on]:bg-accent data-[state=on]:text-on-accent";
const select = "rounded-md border border-border bg-surface px-2 py-1 text-sm";
</script>

<template>
	<section class="flex flex-col gap-3" :aria-label="t('batch.steps.operation')">
		<div class="flex flex-wrap items-center gap-4">
			<h3 class="font-bold">
				<span class="mr-2 rounded-full bg-accent px-2 text-sm text-on-accent">2</span>{{ t("batch.steps.operation") }}
			</h3>
			<div role="group" :aria-label="t('batch.operation.source')">
				<RekaToggleGroupRoot
					type="single"
					:model-value="source"
					class="inline-flex rounded-md border border-border p-0.5 text-sm"
					@update:model-value="(value) => (value === 'rules' || value === 'preset') && (source = value)"
				>
					<RekaToggleGroupItem value="rules" :class="segment">{{ t("batch.operation.rules") }}</RekaToggleGroupItem>
					<RekaToggleGroupItem value="preset" :class="segment">{{ t("batch.operation.preset") }}</RekaToggleGroupItem>
				</RekaToggleGroupRoot>
			</div>
		</div>

		<template v-if="source === 'preset'">
			<p v-if="presets.length === 0" class="text-sm text-muted">
				{{ t("batch.operation.noPresets") }}
				<RouterLink to="/presets" class="text-accent underline">{{ t("batch.operation.manage") }}</RouterLink>
			</p>
			<div v-else class="flex flex-wrap items-start gap-4">
				<label class="flex flex-col gap-1 text-sm">
					{{ t("batch.operation.preset") }}
					<select v-model="presetId" :class="select">
						<option value="" disabled>{{ t("batch.operation.choosePreset") }}</option>
						<option v-for="entry in presets" :key="entry.id" :value="entry.id">{{ entry.name }}</option>
					</select>
				</label>
				<p v-if="preset" class="self-end pb-1 text-sm text-muted">
					<span v-if="presetRules">{{ presetRules }} · </span>
					<span v-else>{{ t("presets.card.noRules") }} · </span>
					<span>{{ containerName(preset.settings?.container ?? 0) }}</span>
					<span class="font-mono"> · {{ batchTemplateOf(preset.settings ?? { template: "", container: 0 }) }}</span>
				</p>
				<RouterLink to="/presets" class="self-end pb-1 text-sm text-accent underline">{{ t("batch.operation.manage") }}</RouterLink>
			</div>
		</template>

		<template v-else>
			<StreamsRules v-model:rules="rules" v-model:untagged="untagged" v-model:video-only="videoOnly" />
			<p v-if="rules.length === 0 && !videoOnly" class="text-sm text-muted">{{ t("batch.operation.remuxNote") }}</p>
		</template>
	</section>
</template>
