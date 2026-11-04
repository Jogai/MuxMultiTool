<script setup lang="ts">
import { type AppError, toAppError } from "~/core/libs/errors";
import { formatTimecode } from "~/core/libs/format";
import { formatBytes } from "~/core/libs/jobFormat";
import { useRpcClient } from "~/core/libs/rpc";
import { type PlanSplitResponse, SplitService } from "~/generated/muxmultitool/cut/v1/split_pb";

import { defaultSplitForm, MAX_PARTS, type SizeUnit, type SplitBy, type SplitError, splitMode } from "../lib/split";

/**
 * The split dialog (sketch 04, "Split…"): by duration, by file size or by number of parts. Preview
 * calls `SplitService.PlanSplit` and lists the parts with their estimated sizes and the warnings; Use
 * replaces the segments with them (the host commits that), so they can be reviewed and edited on the
 * timeline before the export.
 */
const open = defineModel<boolean>("open", { default: false });
const { fileId, output } = defineProps<{
	fileId: string;
	/** The output template the planned operation carries; only echoed by the server. */
	output: string;
}>();
const emit = defineEmits<{ apply: [parts: ReadonlyArray<{ start: number; end: number }>] }>();
const { t } = useI18n();
const client = useRpcClient(SplitService);

const form = ref(defaultSplitForm());
const problem = ref<SplitError>();
const result = shallowRef<PlanSplitResponse>();
const planning = ref(false);
const error = ref<AppError>();

const modes: SplitBy[] = ["duration", "size", "count"];
const units: SizeUnit[] = ["MB", "GB"];

watch(open, (shown) => {
	if (shown) {
		result.value = undefined;
		error.value = undefined;
		problem.value = undefined;
	}
});
watch(
	form,
	() => {
		result.value = undefined;
		problem.value = undefined;
	},
	{ deep: true }
);

async function preview(): Promise<void> {
	const mode = splitMode(form.value);
	if ("error" in mode) {
		problem.value = mode.error;
		return;
	}
	problem.value = undefined;
	error.value = undefined;
	planning.value = true;
	try {
		result.value = await client.planSplit({ fileId, mode: mode.mode, output });
	} catch (cause) {
		result.value = undefined;
		error.value = toAppError(cause);
	} finally {
		planning.value = false;
	}
}

function apply(): void {
	if (!result.value) {
		return;
	}
	emit(
		"apply",
		result.value.parts.map((part) => ({ start: part.startSeconds, end: part.endSeconds }))
	);
	open.value = false;
}

const field = "rounded-md border border-border bg-surface px-2 py-1 text-sm focus-visible:outline-2 focus-visible:outline-accent";
</script>

<template>
	<RekaDialogRoot v-model:open="open">
		<RekaDialogPortal>
			<RekaDialogOverlay class="fixed inset-0 bg-overlay" />
			<RekaDialogContent
				class="fixed top-1/2 left-1/2 flex max-h-[calc(100vh-2rem)] w-[min(36rem,calc(100vw-2rem))] -translate-1/2 flex-col gap-4 overflow-y-auto rounded-lg border border-border bg-surface p-5 shadow-lg"
			>
				<div>
					<RekaDialogTitle class="text-lg font-bold">{{ t("cut.split.title") }}</RekaDialogTitle>
					<RekaDialogDescription class="mt-1 text-sm text-muted">{{ t("cut.split.body") }}</RekaDialogDescription>
				</div>

				<fieldset class="flex flex-col gap-2">
					<legend class="sr-only">{{ t("cut.split.by") }}</legend>
					<label v-for="mode in modes" :key="mode" class="flex flex-wrap items-center gap-2 text-sm">
						<input v-model="form.by" type="radio" class="accent-accent" :value="mode" />
						<span class="w-36">{{ t(`cut.split.modes.${mode}`) }}</span>
						<template v-if="mode === 'duration'">
							<input
								v-model="form.duration"
								type="text"
								:class="[field, 'w-28 font-mono']"
								:disabled="form.by !== 'duration'"
								spellcheck="false"
							/>
						</template>
						<template v-else-if="mode === 'size'">
							<input v-model.number="form.size" type="number" min="1" step="any" :class="[field, 'w-24']" :disabled="form.by !== 'size'" />
							<select v-model="form.sizeUnit" :class="field" :disabled="form.by !== 'size'">
								<option v-for="unit in units" :key="unit" :value="unit">{{ unit }}</option>
							</select>
						</template>
						<template v-else>
							<input
								v-model.number="form.count"
								type="number"
								min="2"
								:max="MAX_PARTS"
								step="1"
								:class="[field, 'w-24']"
								:disabled="form.by !== 'count'"
							/>
						</template>
					</label>
					<p v-if="problem" class="text-xs text-danger">{{ t(`cut.split.errors.${problem}`) }}</p>
				</fieldset>

				<ErrorPanel v-if="error" :error="error" />
				<section v-else-if="result" class="flex flex-col gap-2 text-sm" :aria-label="t('cut.split.preview')">
					<p class="text-xs text-muted">
						{{
							t(
								"cut.split.summary",
								{ n: result.parts.length, duration: formatTimecode(result.durationSeconds), size: formatBytes(result.sizeBytes) },
								result.parts.length
							)
						}}
					</p>
					<ul v-if="result.warnings.length > 0" class="flex flex-col gap-1 text-xs text-warning">
						<li v-for="warning in result.warnings" :key="warning">{{ warning }}</li>
					</ul>
					<div class="max-h-60 overflow-y-auto">
						<table class="w-full font-mono text-xs tabular-nums">
							<thead class="text-left text-muted">
								<tr>
									<th scope="col" class="py-1 pr-2 font-normal">#</th>
									<th scope="col" class="py-1 pr-2 font-normal">{{ t("cut.segments.start") }}</th>
									<th scope="col" class="py-1 pr-2 font-normal">{{ t("cut.segments.end") }}</th>
									<th scope="col" class="py-1 pr-2 font-normal">{{ t("cut.segments.length") }}</th>
									<th scope="col" class="py-1 text-right font-normal">{{ t("cut.split.estimated") }}</th>
								</tr>
							</thead>
							<tbody>
								<tr v-for="part in result.parts" :key="part.number" class="border-t border-border">
									<td class="py-1 pr-2 text-muted">{{ part.number }}</td>
									<td class="py-1 pr-2">{{ formatTimecode(part.startSeconds) }}</td>
									<td class="py-1 pr-2">{{ formatTimecode(part.endSeconds) }}</td>
									<td class="py-1 pr-2">{{ formatTimecode(part.endSeconds - part.startSeconds) }}</td>
									<td class="py-1 text-right">{{ part.estimatedBytes > 0n ? formatBytes(part.estimatedBytes) : "–" }}</td>
								</tr>
							</tbody>
						</table>
					</div>
				</section>

				<div class="flex flex-wrap justify-end gap-2">
					<RekaDialogClose
						class="rounded-md border border-border px-3 py-1.5 text-sm hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent"
					>
						{{ t("cut.split.cancel") }}
					</RekaDialogClose>
					<button
						type="button"
						class="rounded-md border border-border px-3 py-1.5 text-sm hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-50"
						:disabled="planning"
						@click="preview"
					>
						{{ planning ? t("cut.split.planning") : t("cut.split.preview") }}
					</button>
					<button
						type="button"
						class="rounded-md bg-accent px-3 py-1.5 text-sm text-on-accent focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-50"
						:disabled="!result || result.parts.length === 0"
						@click="apply"
					>
						{{ t("cut.split.apply", { n: result?.parts.length ?? 0 }, result?.parts.length ?? 0) }}
					</button>
				</div>
			</RekaDialogContent>
		</RekaDialogPortal>
	</RekaDialogRoot>
</template>
