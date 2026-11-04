<script setup lang="ts">
import { useGifEstimate } from "../composables/useGifEstimate";
import { useImageJob } from "../composables/useImageJob";
import {
	canCreateGif,
	DURATION_LIMITS_S,
	FPS_OPTIONS,
	type GifForm,
	gifName,
	gifRange,
	SIZE_LIMITS_MB,
	toGifOperation,
	WIDTH_LIMITS
} from "../lib/gif";
import type { ImageSupport } from "../lib/support";
import { formatTimecode } from "~/core/libs/format";
import { formatBytes, formatSpan } from "~/core/libs/jobFormat";
import { GifQuality } from "~/generated/muxmultitool/gif/v1/gif_pb";
import type { PlayerRange } from "~/features/player/lib/range";

/**
 * Video → GIF panel (sketch 08): the range from the player, the limits the user states (duration,
 * width, frame rate, file size), the live estimate with every limit it breaks and what to lower,
 * quality and loop, the command, and the submit, which the server refuses again over a limit.
 */
const { fileId, fileName, range, duration, support } = defineProps<{
	fileId: string;
	fileName: string;
	range: PlayerRange;
	duration: number;
	support: ImageSupport;
}>();
const { t } = useI18n();
const jobs = useJobsStore();
const { job, submitting, error, submit } = useImageJob((id) => jobs.byId(id));

const maxDurationS = ref<number>(10);
const maxWidth = ref<number>(480);
const fps = ref<number>(12);
const maxMb = ref<number>(8);
const quality = ref<GifQuality>(GifQuality.BALANCED);
const loop = ref(true);
const output = ref("");
const outputName = computed(() => output.value.trim() || gifName(fileName));

const span = computed(() => gifRange(range, duration, maxDurationS.value));
const form = computed<GifForm>(() => ({
	input: fileId,
	output: outputName.value,
	startMs: span.value.startMs,
	endMs: span.value.endMs,
	fps: fps.value,
	quality: quality.value,
	loop: loop.value,
	maxDurationS: maxDurationS.value,
	maxWidth: maxWidth.value,
	maxBytes: maxMb.value * 1_000_000
}));
const { estimate, estimating, error: estimateError, refresh } = useGifEstimate(form);

const startable = computed(() => canCreateGif(estimate.value, support) && !estimating.value && !submitting.value);
const qualities = [
	{ value: GifQuality.SMALL, key: "small" },
	{ value: GifQuality.BALANCED, key: "balanced" },
	{ value: GifQuality.BEST, key: "best" }
] as const;

async function create() {
	await submit({ case: "videoToGif", value: toGifOperation(form.value) });
	refresh();
}
</script>

<template>
	<section class="flex flex-col gap-4" :aria-label="t('images.tabs.gif')">
		<p class="text-sm text-muted">{{ t("images.gif.description") }}</p>
		<p v-if="support.gifMissing.length > 0" class="text-sm text-danger">
			{{ t("images.gif.unsupported", { missing: support.gifMissing.join(", ") }) }}
		</p>

		<div class="text-sm">
			<p>
				{{
					t("images.gif.range", {
						start: formatTimecode(Number(span.startMs) / 1000),
						end: formatTimecode(Number(span.endMs) / 1000),
						duration: formatSpan(span.endMs - span.startMs)
					})
				}}
			</p>
			<p class="text-xs text-muted">{{ t("images.gif.rangeHint") }}</p>
		</div>

		<fieldset class="grid max-w-xl grid-cols-2 gap-x-4 gap-y-2 text-sm">
			<legend class="mb-1 font-bold">{{ t("images.gif.limits") }}</legend>
			<label class="flex flex-col gap-1">
				{{ t("images.gif.maxDuration") }}
				<select v-model.number="maxDurationS" class="rounded-md border border-border bg-surface px-2 py-1.5">
					<option v-for="seconds in DURATION_LIMITS_S" :key="seconds" :value="seconds">
						{{ t("images.gif.seconds", { n: seconds }) }}
					</option>
				</select>
			</label>
			<label class="flex flex-col gap-1">
				{{ t("images.gif.maxWidth") }}
				<select v-model.number="maxWidth" class="rounded-md border border-border bg-surface px-2 py-1.5">
					<option v-for="width in WIDTH_LIMITS" :key="width" :value="width">{{ t("images.gif.px", { n: width }) }}</option>
				</select>
			</label>
			<label class="flex flex-col gap-1">
				{{ t("images.gif.fps") }}
				<select v-model.number="fps" class="rounded-md border border-border bg-surface px-2 py-1.5">
					<option v-for="rate in FPS_OPTIONS" :key="rate" :value="rate">{{ t("images.gif.fpsValue", { n: rate }) }}</option>
				</select>
			</label>
			<label class="flex flex-col gap-1">
				{{ t("images.gif.maxSize") }}
				<select v-model.number="maxMb" class="rounded-md border border-border bg-surface px-2 py-1.5">
					<option v-for="mb in SIZE_LIMITS_MB" :key="mb" :value="mb">{{ t("images.gif.mb", { n: mb }) }}</option>
				</select>
			</label>
		</fieldset>

		<div class="text-sm" aria-live="polite">
			<ErrorPanel v-if="estimateError" :error="estimateError" />
			<template v-else-if="estimate?.estimate">
				<p>
					{{
						t("images.gif.estimate", {
							width: estimate.estimate.width,
							height: estimate.estimate.height,
							frames: estimate.estimate.frames,
							size: formatBytes(estimate.estimate.estimatedBytes)
						})
					}}
					<span v-if="estimating" class="text-muted"> · {{ t("images.gif.estimating") }}</span>
				</p>
				<p v-if="estimate.violations.length === 0" class="text-success">{{ t("images.gif.within") }}</p>
				<ul v-else class="text-danger">
					<li v-for="violation in estimate.violations" :key="violation.limit">✕ {{ violation.message }}</li>
				</ul>
			</template>
			<p v-else class="text-muted">{{ t("images.gif.estimating") }}</p>
		</div>

		<div class="flex flex-wrap items-center gap-4 text-sm">
			<RekaRadioGroupRoot :model-value="quality" class="flex items-center gap-2" @update:model-value="quality = $event as GifQuality">
				<span class="font-bold">{{ t("images.gif.quality") }}</span>
				<RekaRadioGroupItem
					v-for="entry in qualities"
					:key="entry.key"
					:value="entry.value"
					class="rounded-md border border-border px-3 py-1 hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent data-[state=checked]:border-accent data-[state=checked]:bg-accent-surface"
				>
					{{ t(`images.gif.${entry.key}`) }}
				</RekaRadioGroupItem>
			</RekaRadioGroupRoot>
			<label class="flex items-center gap-2">
				<input v-model="loop" type="checkbox" class="accent-accent" />
				{{ t("images.gif.loop") }}
			</label>
		</div>

		<label class="flex max-w-xl flex-col gap-1 text-sm">
			{{ t("images.output.label") }}
			<input v-model="output" type="text" class="rounded-md border border-border bg-surface px-2 py-1.5" :placeholder="gifName(fileName)" />
			<span class="text-xs text-muted">{{ t("images.output.hint") }}</span>
		</label>

		<div v-if="estimate?.command">
			<h4 class="mb-1 text-sm font-bold">{{ t("images.output.command") }}</h4>
			<pre class="overflow-x-auto rounded-md border border-border bg-surface-sunken p-3 text-xs whitespace-pre-wrap">{{
				estimate.command
			}}</pre>
		</div>

		<div>
			<button
				type="button"
				class="rounded-md bg-accent px-4 py-1.5 text-sm text-on-accent focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-50"
				:disabled="!startable"
				@click="create"
			>
				{{ t("images.gif.create") }}
			</button>
		</div>

		<ErrorPanel v-if="error" :error="error" />
		<ImagesOutput v-if="job" :job="job" :input-id="fileId" :output="outputName" />
	</section>
</template>
