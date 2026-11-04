<script setup lang="ts">
import { useImageJob } from "../composables/useImageJob";
import {
	aspectLabel,
	canCrop,
	type CropEdge,
	croppedSize,
	type CropValues,
	isEmptyCrop,
	sameCrop,
	toCropOperation,
	toCropValues,
	withEdge
} from "../lib/crop";
import { croppedName, toMs } from "../lib/outputs";
import { type AppError, toAppError } from "~/core/libs/errors";
import { formatTimecode } from "~/core/libs/format";
import { useRpcClient } from "~/core/libs/rpc";
import { CropService, type PlanCropResponse } from "~/generated/muxmultitool/crop/v1/crop_pb";

/**
 * Crop panel (sketch 07): auto-detect from the playhead, the four values snapped to the stream's
 * crop unit, the result size, the always-visible limitations, the command and the submit. The
 * values, the output name and the plan live in the Crop page, because the overlay on the player
 * shares them.
 */
const { fileId, fileName, position, plan, planning, planError } = defineProps<{
	fileId: string;
	fileName: string;
	position: number;
	plan?: PlanCropResponse;
	planning: boolean;
	planError?: AppError;
}>();
const crop = defineModel<CropValues>("crop", { required: true });
const output = defineModel<string>("output", { required: true });
const { t } = useI18n();
const jobs = useJobsStore();
const cropClient = useRpcClient(CropService);
const { job, submitting, error, submit } = useImageJob((id) => jobs.byId(id));

const source = computed(() => plan?.source);
const supported = computed(() => plan?.support?.supported);
const result = computed(() => (source.value ? croppedSize(crop.value, source.value) : undefined));
const outputName = computed(() => output.value.trim() || croppedName(fileName));
const startable = computed(() => canCrop(supported.value, crop.value) && !submitting.value);
const edges: readonly CropEdge[] = ["top", "bottom", "left", "right"];

function setEdge(edge: CropEdge, event: Event) {
	const value = Number((event.target as HTMLInputElement).value);
	if (!Number.isFinite(value)) {
		return;
	}
	const next = source.value ? withEdge(crop.value, edge, value, source.value) : { ...crop.value, [edge]: Math.max(0, value) };
	// The browser fires change again when the field loses focus after the snap rewrote it; an equal crop needs no new plan
	if (!sameCrop(next, crop.value)) {
		crop.value = next;
	}
}

// Black-bar detection from the playhead on
const detecting = ref(false);
const detectError = ref<AppError>();
const found = ref<{ values: CropValues; frames: number }>();
async function detect() {
	detecting.value = true;
	detectError.value = undefined;
	found.value = undefined;
	try {
		const response = await cropClient.detectBlackBars({ input: fileId, positionMs: toMs(position) });
		const values = toCropValues(response.crop);
		found.value = { values, frames: response.framesScanned };
		crop.value = values;
	} catch (cause) {
		detectError.value = toAppError(cause);
	} finally {
		detecting.value = false;
	}
}

async function start() {
	await submit({ case: "crop", value: toCropOperation(fileId, outputName.value, crop.value) });
}
</script>

<template>
	<section class="flex flex-col gap-4" :aria-label="t('nav.crop')">
		<h3 class="font-bold">
			<template v-if="source">{{ t("images.crop.title", { codec: source.codec, width: source.width, height: source.height }) }}</template>
			<template v-else>{{ t("nav.crop") }}</template>
		</h3>

		<ErrorPanel v-if="planError" :error="planError" />
		<p v-else-if="supported === false" class="text-sm text-danger">{{ t("images.crop.unsupported", { reason: plan?.support?.reason }) }}</p>

		<div class="flex flex-col gap-1 text-sm">
			<div class="flex flex-wrap items-center gap-3">
				<button
					type="button"
					class="rounded-md border border-border px-3 py-1.5 hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-50"
					:disabled="detecting || supported === false"
					@click="detect"
				>
					{{ detecting ? t("images.crop.detecting") : t("images.crop.detect") }}
				</button>
				<span class="text-xs text-muted">{{ t("images.position", { time: formatTimecode(position) }) }}</span>
			</div>
			<p v-if="found" role="status">
				{{ t("images.crop.found", { ...found.values, frames: found.frames }) }}
			</p>
			<p class="text-xs text-muted">{{ t("images.crop.detectHint") }}</p>
			<ErrorPanel v-if="detectError" :error="detectError" />
		</div>

		<fieldset class="grid max-w-md grid-cols-2 gap-x-4 gap-y-2 text-sm sm:grid-cols-4">
			<legend class="sr-only">{{ t("nav.crop") }}</legend>
			<label v-for="edge in edges" :key="edge" class="flex flex-col gap-1">
				{{ t(`images.crop.${edge}`) }}
				<span class="flex items-center gap-1">
					<input
						type="number"
						min="0"
						:step="edge === 'top' || edge === 'bottom' ? source?.unitY || 1 : source?.unitX || 1"
						:value="crop[edge]"
						:disabled="supported === false"
						class="w-full rounded-md border border-border bg-surface px-2 py-1.5 text-right tabular-nums"
						@change="setEdge(edge, $event)"
					/>
					<span class="text-muted">px</span>
				</span>
			</label>
		</fieldset>
		<p v-if="source" class="text-xs text-muted">
			{{
				source.unitX === source.unitY
					? t("images.crop.unitSame", { n: source.unitX, pixFmt: source.pixFmt })
					: t("images.crop.unit", { x: source.unitX, y: source.unitY, pixFmt: source.pixFmt })
			}}
		</p>

		<p v-if="source && result" class="text-sm">
			<span class="font-bold">{{ t("images.crop.result", result) }}</span>
			<span class="ml-1 text-muted">
				{{ t("images.crop.from", { width: source.width, height: source.height, aspect: aspectLabel(result.width, result.height) }) }}</span
			>
			<span v-if="planning" class="ml-1 text-muted">· {{ t("images.crop.planning") }}</span>
		</p>
		<p v-if="supported && isEmptyCrop(crop)" class="text-sm text-muted">{{ t("images.crop.nothing") }}</p>

		<aside
			v-if="plan?.limitations.length"
			class="rounded-md border border-warning bg-surface-raised p-3 text-sm"
			:aria-label="t('images.crop.limitations')"
		>
			<h4 class="mb-1 font-bold">{{ t("images.crop.limitations") }}</h4>
			<ul class="list-disc pl-5">
				<li v-for="note in plan.limitations" :key="note">{{ note }}</li>
			</ul>
		</aside>

		<label class="flex max-w-xl flex-col gap-1 text-sm">
			{{ t("images.output.label") }}
			<input
				v-model="output"
				type="text"
				class="rounded-md border border-border bg-surface px-2 py-1.5"
				:placeholder="croppedName(fileName)"
				:disabled="supported === false"
			/>
			<span class="text-xs text-muted">{{ t("images.output.hint") }}</span>
		</label>

		<div v-if="plan?.command">
			<h4 class="mb-1 text-sm font-bold">{{ t("images.output.command") }}</h4>
			<pre class="overflow-x-auto rounded-md border border-border bg-surface-sunken p-3 text-xs whitespace-pre-wrap">{{
				plan.command
			}}</pre>
		</div>

		<div>
			<button
				type="button"
				class="rounded-md bg-accent px-4 py-1.5 text-sm text-on-accent focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-50"
				:disabled="!startable"
				@click="start"
			>
				{{ t("images.crop.start") }}
			</button>
		</div>

		<ErrorPanel v-if="error" :error="error" />
		<ImagesOutput v-if="job" :job="job" :input-id="fileId" :output="outputName" />
	</section>
</template>
