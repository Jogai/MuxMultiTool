<script setup lang="ts">
import IconX from "~icons/lucide/x";

import { isAdded, isOffsetValid, type SourceDraft, sourceLabel, type TrackDraft } from "../lib/combine";
import type { CachedReport } from "~/core/composables/useStreamReport";
import { formatDuration, streamTypeName } from "~/core/libs/reportFormat";
import type { StreamSummary } from "~/generated/muxmultitool/files/v1/files_pb";

/**
 * The sources column of the sketch: every file with its label, duration and streams; "+ add" copies
 * a stream into the output and an added stream stays marked. A source can be shifted in time, which
 * moves the timestamps of its tracks and nothing else (the copy stays lossless).
 */
const { sources, tracks, reports } = defineProps<{
	sources: readonly SourceDraft[];
	tracks: readonly TrackDraft[];
	/** The quick reports by file id; a source without one is still loading. */
	reports: ReadonlyMap<string, CachedReport>;
}>();
const emit = defineEmits<{
	add: [source: number, streamIndex: number];
	remove: [position: number];
	offset: [position: number, value: string];
	addSource: [];
}>();
const { t } = useI18n();

const reportOf = (source: SourceDraft) => reports.get(source.id)?.report;
const durationOf = (source: SourceDraft) => {
	const seconds = reportOf(source)?.container?.durationSeconds;
	return seconds ? formatDuration(seconds) : undefined;
};

/** `hevc 2160p`, `truehd 7.1`, `srt`. */
function streamLabel(stream: StreamSummary): string {
	if (stream.height > 0) {
		return `${stream.codec} ${stream.height}p`;
	}
	if (stream.channelLayout) {
		return `${stream.codec} ${stream.channelLayout}`;
	}
	return stream.codec;
}

const addButton =
	"rounded-md border border-border px-2 py-0.5 text-xs hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent";
</script>

<template>
	<section class="flex min-w-0 flex-col gap-3" :aria-label="t('combine.sources.title')">
		<h3 class="font-bold">{{ t("combine.sources.title") }}</h3>
		<p v-if="sources.length === 0" class="text-sm text-muted">{{ t("combine.sources.empty") }}</p>
		<article
			v-for="(source, position) in sources"
			:key="source.id"
			class="rounded-md border border-border"
			:aria-label="`${sourceLabel(position)} ${source.name}`"
		>
			<header class="flex items-center gap-2 bg-surface-raised px-3 py-1.5 text-sm">
				<span class="rounded bg-accent px-1.5 font-mono text-xs font-bold text-on-accent">{{ sourceLabel(position) }}</span>
				<span class="min-w-0 flex-1 truncate font-bold" :title="source.name">{{ source.name }}</span>
				<span class="shrink-0 text-xs text-muted">
					<template v-if="durationOf(source)">{{ durationOf(source) }} · </template>
					<template v-if="reportOf(source)">{{
						t("files.report.streams", { n: reportOf(source)?.streams.length ?? 0 }, reportOf(source)?.streams.length ?? 0)
					}}</template>
					<template v-else>{{ t("combine.sources.loading") }}</template>
				</span>
				<button
					type="button"
					class="rounded-md p-1 hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent"
					:aria-label="t('combine.sources.remove', { name: source.name })"
					@click="emit('remove', position)"
				>
					<IconX class="size-4" aria-hidden="true" />
				</button>
			</header>
			<p v-if="reportOf(source)?.error" class="px-3 py-2 text-sm text-danger">{{ reportOf(source)?.error?.message }}</p>
			<ul v-else class="divide-y divide-border">
				<li
					v-for="stream in reportOf(source)?.streams ?? []"
					:key="stream.index"
					class="flex items-center gap-3 px-3 py-1 text-sm"
					:class="{ 'bg-accent-surface': isAdded(tracks, position, stream.index) }"
				>
					<span class="w-8 font-mono text-muted">#{{ stream.index }}</span>
					<span class="w-16">{{ streamTypeName(stream.type) }}</span>
					<span class="min-w-0 flex-1 truncate font-mono">{{ streamLabel(stream) }}</span>
					<span class="w-10 font-mono">{{ stream.language }}</span>
					<span class="min-w-0 max-w-40 truncate text-muted" :title="stream.title || undefined">{{ stream.title }}</span>
					<span v-if="isAdded(tracks, position, stream.index)" class="ml-auto text-xs text-success">{{ t("combine.sources.added") }}</span>
					<button v-else type="button" :class="addButton" class="ml-auto" @click="emit('add', position, stream.index)">
						{{ t("combine.sources.add") }}
					</button>
				</li>
			</ul>
			<label class="flex items-center gap-2 border-t border-border px-3 py-1.5 text-xs text-muted">
				{{ t("combine.sources.offset", { label: sourceLabel(position) }) }}
				<input
					:value="source.offset"
					type="text"
					inputmode="decimal"
					class="w-20 rounded-md border border-border bg-surface px-2 py-0.5 font-mono text-text"
					:class="{ 'border-danger': !isOffsetValid(source.offset) }"
					:aria-invalid="!isOffsetValid(source.offset)"
					placeholder="0"
					@input="emit('offset', position, ($event.target as HTMLInputElement).value)"
				/>
				s
			</label>
		</article>
		<button
			type="button"
			class="self-start rounded-md border border-border px-3 py-1.5 text-sm hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent"
			@click="emit('addSource')"
		>
			{{ t("combine.sources.addSource") }}
		</button>
	</section>
</template>
