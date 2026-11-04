<script setup lang="ts">
import type { AppError } from "~/core/libs/errors";
import { audioLabel, formatBitRate, formatDuration, formatStreamSize, streamTypeName } from "~/core/libs/reportFormat";
import { type GetStreamDetailsResponse, SizeMethod, type StreamSummary } from "~/generated/muxmultitool/files/v1/files_pb";

/**
 * The details panel of the sketch: what the report knows about the chosen stream at a glance, then
 * every ffprobe field, and the raw JSON one click away. The fields are fetched when the panel opens
 * on a stream, not with the report.
 */
const { fileId, fileName, summary, fetch } = defineProps<{
	fileId: string;
	fileName: string;
	summary: StreamSummary;
	fetch: (fileId: string, streamIndex: number) => Promise<GetStreamDetailsResponse>;
}>();
const emit = defineEmits<{ close: [] }>();
const { t } = useI18n();

const details = shallowRef<GetStreamDetailsResponse>();
const loading = ref(false);
const error = ref<AppError>();
const jsonOpen = ref(false);
const clipboard = useClipboard();

watch(
	() => [fileId, summary.index] as const,
	async ([id, index]) => {
		details.value = undefined;
		error.value = undefined;
		loading.value = true;
		try {
			const response = await fetch(id, index);
			if (id === fileId && index === summary.index) {
				details.value = response;
			}
		} catch (cause) {
			if (id === fileId && index === summary.index) {
				error.value = toAppError(cause);
			}
		} finally {
			if (id === fileId && index === summary.index) {
				loading.value = false;
			}
		}
	},
	{ immediate: true }
);

/** The at-a-glance figures: only what applies to this stream. */
const figures = computed(() => {
	const rows: { key: string; value: string; title?: string }[] = [];
	const push = (key: string, value: string | undefined, title?: string) => {
		if (value) {
			rows.push({ key, value, title });
		}
	};
	push("type", streamTypeName(summary.type));
	push("codec", summary.codecLongName ? `${summary.codec} · ${summary.codecLongName}` : summary.codec);
	push("profile", summary.profile);
	push("language", summary.language);
	push("title", summary.title);
	push("size", `${formatStreamSize(summary)} GB`, t(`files.report.sizeMethod.${SizeMethod[summary.sizeMethod].toLowerCase()}`));
	push("duration", formatDuration(summary.durationSeconds));
	push("bitRate", formatBitRate(summary.bitRate));
	if (summary.width > 0 || summary.height > 0) {
		push(
			"video",
			[
				`${summary.width}×${summary.height}`,
				summary.frameRate > 0 ? `${summary.frameRate.toFixed(3).replace(/\.?0+$/, "")} fps` : "",
				summary.pixFmt
			]
				.filter(Boolean)
				.join(" · ")
		);
	}
	push("audio", audioLabel(summary));
	push("flags", summary.flags.join(", "));
	return rows;
});
</script>

<template>
	<aside
		class="flex min-w-0 flex-col rounded-lg border border-border bg-surface-raised"
		:aria-label="t('files.details.title', { name: fileName, index: summary.index })"
	>
		<header class="flex items-center gap-3 border-b border-border px-4 py-3">
			<h3 class="min-w-0 truncate font-bold" :title="fileName">{{ t("files.details.title", { name: fileName, index: summary.index }) }}</h3>
			<button
				type="button"
				class="ml-auto flex size-8 shrink-0 items-center justify-center rounded-md text-muted hover:bg-surface-sunken hover:text-text focus-visible:outline-2 focus-visible:outline-accent"
				:aria-label="t('files.details.close')"
				@click="emit('close')"
			>
				<i-ph-x class="size-4" />
			</button>
		</header>

		<dl class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 px-4 py-3 text-sm">
			<template v-for="figure in figures" :key="figure.key">
				<dt class="text-xs text-muted">{{ t(`files.details.figures.${figure.key}`) }}</dt>
				<dd class="min-w-0 font-mono break-words" :title="figure.title">{{ figure.value }}</dd>
			</template>
		</dl>

		<section class="border-t border-border px-4 py-3 text-sm" :aria-busy="loading">
			<h4 class="text-xs text-muted">{{ t("files.details.fields") }}</h4>
			<p v-if="loading" class="mt-1 text-muted">{{ t("files.details.loading") }}</p>
			<ErrorPanel v-else-if="error" :error="error" class="mt-2" />
			<dl v-else-if="details" class="mt-1 grid max-h-96 grid-cols-[auto_1fr] gap-x-4 gap-y-0.5 overflow-auto font-mono text-xs">
				<template v-for="field in details.fields" :key="field.key">
					<dt class="text-muted">{{ field.key }}</dt>
					<dd class="min-w-0 break-all">{{ field.value }}</dd>
				</template>
			</dl>
		</section>

		<RekaCollapsibleRoot v-if="details?.json" v-model:open="jsonOpen" class="border-t border-border px-4 py-3 text-sm">
			<div class="flex items-center gap-3">
				<RekaCollapsibleTrigger class="text-xs text-muted underline focus-visible:outline-2 focus-visible:outline-accent">
					{{ jsonOpen ? t("files.details.hideJson") : t("files.details.showJson") }}
				</RekaCollapsibleTrigger>
				<button
					type="button"
					class="ml-auto rounded-md border border-border px-2 py-1 text-xs hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent"
					@click="clipboard.copy(details.json)"
				>
					{{ clipboard.copied.value ? t("files.details.copied") : t("files.details.copyJson") }}
				</button>
			</div>
			<RekaCollapsibleContent>
				<pre class="mt-2 max-h-96 overflow-auto rounded bg-surface-sunken p-2 font-mono text-xs" :aria-label="t('files.details.json')">{{
					details.json
				}}</pre>
			</RekaCollapsibleContent>
		</RekaCollapsibleRoot>
	</aside>
</template>
