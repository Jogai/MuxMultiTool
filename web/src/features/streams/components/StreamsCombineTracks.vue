<script setup lang="ts">
import IconArrowDown from "~icons/lucide/arrow-down";
import IconArrowUp from "~icons/lucide/arrow-up";
import IconGripVertical from "~icons/lucide/grip-vertical";
import IconX from "~icons/lucide/x";

import { originOf, type SourceDraft, type TrackDraft } from "../lib/combine";
import { useDragReorder } from "~/core/composables/useDragReorder";
import type { CachedReport } from "~/core/composables/useStreamReport";
import { streamTypeName } from "~/core/libs/reportFormat";
import type { OutputTrack, TrackIssue } from "~/generated/muxmultitool/combine/v1/combine_pb";
import type { StreamSummary } from "~/generated/muxmultitool/files/v1/files_pb";

/**
 * The output tracks of the sketch, in output order: drag or use the buttons to reorder (the keyboard
 * path), and edit language, title and the default/forced flags per track; the edits become
 * `-metadata`/`-disposition` options. The values shown are what the output will carry, from the
 * preview, so a track that is not edited shows what its source has.
 */
const { tracks, sources, reports, outputs, issues } = defineProps<{
	tracks: readonly TrackDraft[];
	sources: readonly SourceDraft[];
	reports: ReadonlyMap<string, CachedReport>;
	/** The preview's tracks; used when they line up with the drafts. */
	outputs: readonly OutputTrack[];
	issues: readonly TrackIssue[];
}>();
const emit = defineEmits<{
	reorder: [tracks: TrackDraft[]];
	remove: [position: number];
	edit: [position: number, patch: Partial<Omit<TrackDraft, "source" | "streamIndex">>];
}>();
/** The row the keyboard shortcuts act on: the one last clicked or focused. */
const active = defineModel<number>("active");
const { t } = useI18n();

const { dragging, moveTo, drop } = useDragReorder(
	() => tracks,
	(next, to) => {
		emit("reorder", next);
		if (to >= 0 && to < tracks.length) {
			active.value = to;
		}
	}
);

function streamOf(track: TrackDraft): StreamSummary | undefined {
	const source = sources[track.source];
	return source ? reports.get(source.id)?.report.streams.find((stream) => stream.index === track.streamIndex) : undefined;
}
const outputOf = (position: number): OutputTrack | undefined => (outputs.length === tracks.length ? outputs[position] : undefined);
const issuesOf = (position: number) => issues.filter((issue) => issue.track === position);

/** `hevc 2160p`, `truehd 7.1`, `srt`. */
function codecLabel(stream: StreamSummary | undefined): string {
	if (!stream) {
		return "";
	}
	if (stream.height > 0) {
		return `${stream.codec} ${stream.height}p`;
	}
	return stream.channelLayout ? `${stream.codec} ${stream.channelLayout}` : stream.codec;
}
const languageOf = (track: TrackDraft, position: number) =>
	track.language ?? outputOf(position)?.language ?? streamOf(track)?.language ?? "";
const titleOf = (track: TrackDraft, position: number) => track.title ?? outputOf(position)?.title ?? streamOf(track)?.title ?? "";
const flagOf = (track: TrackDraft, position: number, flag: "default" | "forced") =>
	track[flag] ?? outputOf(position)?.[flag] ?? streamOf(track)?.flags.includes(flag) ?? false;

const field = "w-full rounded-md border border-border bg-surface px-1.5 py-0.5 text-sm";
const button =
	"rounded-md p-1 hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-40 disabled:hover:bg-transparent";
</script>

<template>
	<section class="flex min-w-0 flex-col gap-2" :aria-label="t('combine.tracks.title')">
		<h3 class="font-bold">
			{{ t("combine.tracks.title") }} <span class="font-normal text-muted">· {{ t("combine.tracks.hint") }}</span>
		</h3>
		<p v-if="tracks.length === 0" class="text-sm text-muted">{{ t("combine.tracks.empty") }}</p>
		<div v-else class="overflow-x-auto">
			<table class="w-full border-collapse text-sm">
				<thead class="text-left text-xs text-muted uppercase">
					<tr class="border-b border-border">
						<th scope="col" class="w-6 py-2" />
						<th scope="col" class="py-2 pr-3 font-normal">{{ t("combine.tracks.track") }}</th>
						<th scope="col" class="py-2 pr-3 font-normal">{{ t("files.report.columns.type") }}</th>
						<th scope="col" class="py-2 pr-3 font-normal">{{ t("files.report.columns.codec") }}</th>
						<th scope="col" class="w-20 py-2 pr-3 font-normal">{{ t("files.report.columns.language") }}</th>
						<th scope="col" class="min-w-32 py-2 pr-3 font-normal">{{ t("files.report.columns.title") }}</th>
						<th scope="col" class="py-2 pr-3 font-normal">{{ t("combine.tracks.default") }}</th>
						<th scope="col" class="py-2 pr-3 font-normal">{{ t("combine.tracks.forced") }}</th>
						<th scope="col" class="py-2 text-right font-normal">{{ t("combine.tracks.actions") }}</th>
					</tr>
				</thead>
				<tbody>
					<template v-for="(track, position) in tracks" :key="`${track.source}:${track.streamIndex}`">
						<tr
							class="border-b border-border align-top"
							:class="{ 'bg-surface-sunken': dragging === position, 'bg-accent-surface': active === position }"
							:aria-selected="active === position"
							@click="active = position"
							@focusin="active = position"
							@dragover.prevent
							@drop.prevent="drop(position)"
						>
							<td class="py-1.5">
								<span
									class="inline-flex cursor-grab text-muted"
									draggable="true"
									:title="t('combine.tracks.drag')"
									@dragstart="dragging = position"
									@dragend="dragging = undefined"
								>
									<IconGripVertical class="size-4" aria-hidden="true" />
								</span>
							</td>
							<td class="py-1.5 pr-3 font-mono whitespace-nowrap">
								{{ position + 1 }} · {{ outputOf(position)?.origin ?? originOf(track) }}
							</td>
							<td class="py-1.5 pr-3">{{ streamTypeName(outputOf(position)?.stream?.type ?? streamOf(track)?.type ?? 0) }}</td>
							<td class="py-1.5 pr-3 font-mono whitespace-nowrap">{{ codecLabel(outputOf(position)?.stream ?? streamOf(track)) }}</td>
							<td class="py-1 pr-3">
								<input
									:value="languageOf(track, position)"
									type="text"
									:class="field"
									class="font-mono"
									maxlength="3"
									:aria-label="t('combine.tracks.languageOf', { track: position + 1 })"
									@input="emit('edit', position, { language: ($event.target as HTMLInputElement).value })"
								/>
							</td>
							<td class="py-1 pr-3">
								<input
									:value="titleOf(track, position)"
									type="text"
									:class="field"
									:aria-label="t('combine.tracks.titleOf', { track: position + 1 })"
									@input="emit('edit', position, { title: ($event.target as HTMLInputElement).value })"
								/>
							</td>
							<td class="py-1.5 pr-3 text-center">
								<input
									type="checkbox"
									class="accent-accent"
									:checked="flagOf(track, position, 'default')"
									:aria-label="t('combine.tracks.defaultOf', { track: position + 1 })"
									@change="emit('edit', position, { default: ($event.target as HTMLInputElement).checked })"
								/>
							</td>
							<td class="py-1.5 pr-3 text-center">
								<input
									type="checkbox"
									class="accent-accent"
									:checked="flagOf(track, position, 'forced')"
									:aria-label="t('combine.tracks.forcedOf', { track: position + 1 })"
									@change="emit('edit', position, { forced: ($event.target as HTMLInputElement).checked })"
								/>
							</td>
							<td class="py-1 text-right whitespace-nowrap">
								<button
									type="button"
									:class="button"
									:disabled="position === 0"
									:aria-label="t('combine.tracks.up', { track: position + 1 })"
									@click.stop="moveTo(position, position - 1)"
								>
									<IconArrowUp class="size-4" aria-hidden="true" />
								</button>
								<button
									type="button"
									:class="button"
									:disabled="position === tracks.length - 1"
									:aria-label="t('combine.tracks.down', { track: position + 1 })"
									@click.stop="moveTo(position, position + 1)"
								>
									<IconArrowDown class="size-4" aria-hidden="true" />
								</button>
								<button
									type="button"
									:class="button"
									:aria-label="t('combine.tracks.remove', { track: position + 1 })"
									@click.stop="emit('remove', position)"
								>
									<IconX class="size-4" aria-hidden="true" />
								</button>
							</td>
						</tr>
						<tr v-if="issuesOf(position).length > 0" class="border-b border-border">
							<td />
							<td colspan="8" class="py-1 text-xs">
								<p v-for="issue in issuesOf(position)" :key="issue.message" :class="issue.blocking ? 'text-danger' : 'text-warning'">
									<span aria-hidden="true">{{ issue.blocking ? "✕" : "⚠" }}</span> {{ issue.message }}
								</p>
							</td>
						</tr>
					</template>
				</tbody>
			</table>
		</div>
	</section>
</template>
