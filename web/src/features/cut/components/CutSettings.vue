<script setup lang="ts">
import type { AppError } from "~/core/libs/errors";
import { formatTimecode } from "~/core/libs/format";
import { type CheckSmartCutResponse, CutExport, CutMode, type PlanCutResponse, SegmentMode } from "~/generated/muxmultitool/cut/v1/cut_pb";

import { jobLink } from "../lib/export";
import type { EditorState } from "../lib/segments";

/**
 * The settings column of sketch 04: keep/remove, the cut mode (keyframe, normal, smart with the reason
 * when the file cannot be smart cut), the export shape and output, the split dialog's button, the plan
 * (where the cuts land, the outputs, the command) and the Export button that submits the job.
 */
const {
	state,
	duration,
	smart,
	smartChecking = false,
	plan,
	planning = false,
	planError,
	output,
	submitting = false,
	submitted
} = defineProps<{
	state: EditorState;
	duration: number;
	smart?: CheckSmartCutResponse;
	smartChecking?: boolean;
	plan?: PlanCutResponse;
	planning?: boolean;
	planError?: AppError;
	output: string;
	submitting?: boolean;
	submitted?: { id: string; name: string };
}>();
const emit = defineEmits<{
	"update:segmentMode": [mode: SegmentMode];
	"update:cutMode": [mode: CutMode];
	"update:export": [mode: CutExport];
	"update:output": [output: string];
	split: [];
	export: [];
}>();
const { t } = useI18n();

const smartSupported = computed(() => smart?.supported ?? false);
const smartSummary = computed(() => {
	if (!plan || state.cutMode !== CutMode.SMART) {
		return undefined;
	}
	const reencoded = plan.segments.reduce((sum, segment) => sum + segment.reencodedSeconds, 0);
	const cutPoints = plan.segments.reduce(
		(count, segment) => count + (segment.cutStartSeconds > 0.001 ? 1 : 0) + (duration > 0 && segment.endSeconds < duration - 0.001 ? 1 : 0),
		0
	);
	return t("cut.mode.smartSummary", { seconds: reencoded.toFixed(1), points: cutPoints, encoder: smart?.encoder || "x264/x265" });
});
const canSubmit = computed(() => plan !== undefined && !planning && !submitting && !planError);

const radio = "flex items-start gap-2 text-sm";
const panel = "flex flex-col gap-2 rounded-md border border-border p-3";
</script>

<template>
	<div class="flex flex-col gap-3">
		<fieldset :class="panel">
			<legend class="px-1 text-sm font-bold">{{ t("cut.mode.segments") }}</legend>
			<label :class="radio">
				<input
					type="radio"
					class="mt-1 accent-accent"
					:checked="state.segmentMode !== SegmentMode.REMOVE"
					@change="emit('update:segmentMode', SegmentMode.KEEP)"
				/>
				<span
					>{{ t("cut.mode.keep") }}<span class="block text-xs text-muted">{{ t("cut.mode.keepHint") }}</span></span
				>
			</label>
			<label :class="radio">
				<input
					type="radio"
					class="mt-1 accent-accent"
					:checked="state.segmentMode === SegmentMode.REMOVE"
					@change="emit('update:segmentMode', SegmentMode.REMOVE)"
				/>
				<span
					>{{ t("cut.mode.remove") }}<span class="block text-xs text-muted">{{ t("cut.mode.removeHint") }}</span></span
				>
			</label>
		</fieldset>

		<fieldset :class="panel">
			<legend class="px-1 text-sm font-bold">{{ t("cut.mode.cut") }}</legend>
			<label :class="radio">
				<input
					type="radio"
					class="mt-1 accent-accent"
					:checked="state.cutMode === CutMode.KEYFRAME"
					@change="emit('update:cutMode', CutMode.KEYFRAME)"
				/>
				<span
					>{{ t("cut.mode.keyframe") }}<span class="block text-xs text-muted">{{ t("cut.mode.keyframeHint") }}</span></span
				>
			</label>
			<label :class="radio">
				<input
					type="radio"
					class="mt-1 accent-accent"
					:checked="state.cutMode === CutMode.NORMAL"
					@change="emit('update:cutMode', CutMode.NORMAL)"
				/>
				<span
					>{{ t("cut.mode.normal") }}<span class="block text-xs text-muted">{{ t("cut.mode.normalHint") }}</span></span
				>
			</label>
			<label :class="radio" :title="smart && !smart.supported ? smart.reason : undefined">
				<input
					type="radio"
					class="mt-1 accent-accent disabled:opacity-50"
					:checked="state.cutMode === CutMode.SMART"
					:disabled="!smartSupported"
					@change="emit('update:cutMode', CutMode.SMART)"
				/>
				<span :class="{ 'text-muted': !smartSupported }">
					{{ t("cut.mode.smart") }}
					<span class="block text-xs text-muted">{{ t("cut.mode.smartHint") }}</span>
					<span v-if="smartChecking" class="block text-xs text-muted">{{ t("cut.mode.smartChecking") }}</span>
					<span v-else-if="smart && !smart.supported" class="block text-xs text-warning">{{
						t("cut.mode.smartUnsupported", { reason: smart.reason })
					}}</span>
					<span v-else-if="smartSummary" class="block text-xs text-text">{{ smartSummary }}</span>
				</span>
			</label>
		</fieldset>

		<fieldset :class="panel">
			<legend class="px-1 text-sm font-bold">{{ t("cut.export.title") }}</legend>
			<label :class="radio">
				<input
					type="radio"
					class="mt-1 accent-accent"
					:checked="state.export !== CutExport.MERGED"
					@change="emit('update:export', CutExport.SEPARATE_FILES)"
				/>
				<span>{{ t("cut.export.separate") }}</span>
			</label>
			<label :class="radio">
				<input
					type="radio"
					class="mt-1 accent-accent"
					:checked="state.export === CutExport.MERGED"
					@change="emit('update:export', CutExport.MERGED)"
				/>
				<span
					>{{ t("cut.export.merged") }}<span class="block text-xs text-muted">{{ t("cut.export.mergedHint") }}</span></span
				>
			</label>
			<label class="flex flex-col gap-1 text-sm">
				{{ t("cut.export.output") }}
				<input
					type="text"
					class="rounded-md border border-border bg-surface px-2 py-1.5 font-mono text-xs"
					:value="output"
					spellcheck="false"
					@input="emit('update:output', ($event.target as HTMLInputElement).value)"
				/>
				<span class="text-xs text-muted">{{
					state.export === CutExport.MERGED ? t("cut.export.outputHintMerged") : t("cut.export.outputHintSeparate")
				}}</span>
			</label>
			<div>
				<button
					type="button"
					class="rounded-md border border-border px-3 py-1.5 text-sm hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent"
					@click="emit('split')"
				>
					{{ t("cut.split.open") }}
				</button>
			</div>
		</fieldset>

		<section :class="panel" :aria-label="t('cut.plan.title')">
			<h3 class="text-sm font-bold">{{ t("cut.plan.title") }}</h3>
			<p v-if="planning" class="text-xs text-muted">{{ t("cut.plan.planning") }}</p>
			<ErrorPanel v-if="planError" :error="planError" />
			<template v-else-if="plan">
				<ul class="flex flex-col gap-0.5 font-mono text-xs tabular-nums">
					<li v-for="segment in plan.segments" :key="segment.number" class="flex flex-wrap gap-x-2">
						<span class="text-muted">{{ segment.number }}.</span>
						<span>{{ formatTimecode(segment.cutStartSeconds) }} – {{ formatTimecode(segment.endSeconds) }}</span>
						<span v-if="Math.abs(segment.cutStartSeconds - segment.startSeconds) > 0.0005" class="text-muted">
							{{ t("cut.plan.snapped", { time: formatTimecode(segment.startSeconds) }) }}
						</span>
					</li>
				</ul>
				<p class="text-xs text-muted">
					{{ t("cut.plan.total", { time: formatTimecode(plan.totalDurationSeconds), n: plan.outputs.length }, plan.outputs.length) }}
				</p>
				<ul class="font-mono text-xs break-all">
					<li v-for="file in plan.outputs" :key="file">{{ file }}</li>
				</ul>
				<details class="text-xs">
					<summary class="cursor-pointer text-muted">{{ t("cut.plan.command") }}</summary>
					<pre class="mt-1 overflow-x-auto rounded-md border border-border bg-surface-sunken p-2 whitespace-pre-wrap">{{
						plan.commands.length > 0 ? plan.commands.join("\n\n") : plan.command
					}}</pre>
				</details>
			</template>
			<p v-else-if="!planning" class="text-xs text-muted">{{ t("cut.plan.none") }}</p>
		</section>

		<div class="flex flex-wrap items-center gap-3">
			<button
				type="button"
				class="rounded-md bg-accent px-4 py-1.5 text-sm text-on-accent focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-50"
				:disabled="!canSubmit"
				@click="emit('export')"
			>
				{{ t("cut.export.start") }}
			</button>
			<span v-if="submitting" class="text-sm text-muted">{{ t("cut.export.submitting") }}</span>
		</div>
		<p v-if="submitted" class="text-sm" role="status">
			{{ t("cut.export.submitted", { name: submitted.name }) }}
			<RouterLink :to="jobLink(submitted.id)" class="text-accent underline">{{ t("cut.export.viewJob") }}</RouterLink>
		</p>
	</div>
</template>
