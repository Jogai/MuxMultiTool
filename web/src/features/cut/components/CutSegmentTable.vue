<script setup lang="ts">
import IconLocate from "~icons/lucide/locate";
import IconTrash from "~icons/lucide/trash";

import { formatTimecode } from "~/core/libs/format";

import { type Segment, type TimeError, validateTimes } from "../lib/segments";

/**
 * The segment list from sketch 04: number, start, end and length per row, with the times editable
 * (see `CutTimeField`). A row is selected by clicking it; the selection is shared with the timeline.
 */
const { segments, selectedId, duration, fileId } = defineProps<{
	segments: readonly Segment[];
	selectedId?: number;
	duration: number;
	fileId: string;
}>();
const emit = defineEmits<{
	"update:selectedId": [id: number | undefined];
	remove: [id: number];
	seek: [seconds: number];
	times: [id: number, start: number, end: number];
}>();
const { t } = useI18n();

/** The problem with the last typed pair per segment, shown under the row until a valid pair is typed. */
const problems = ref<Record<number, TimeError>>({});

function setTime(segment: Segment, edge: "start" | "end", seconds: number): void {
	const start = edge === "start" ? seconds : segment.start;
	const end = edge === "end" ? seconds : segment.end;
	const problem = validateTimes(start, end, duration);
	if (problem) {
		problems.value = { ...problems.value, [segment.id]: problem };
		return;
	}
	const { [segment.id]: _, ...rest } = problems.value;
	problems.value = rest;
	emit("times", segment.id, start, end);
}
</script>

<template>
	<div class="flex flex-col gap-2">
		<p v-if="segments.length === 0" class="text-sm text-muted">{{ t("cut.segments.empty") }}</p>
		<table v-else class="w-full text-sm">
			<thead class="text-left text-xs text-muted">
				<tr>
					<th scope="col" class="py-1 pr-2 font-normal">#</th>
					<th scope="col" class="py-1 pr-2 font-normal">{{ t("cut.segments.start") }}</th>
					<th scope="col" class="py-1 pr-2 font-normal">{{ t("cut.segments.end") }}</th>
					<th scope="col" class="py-1 pr-2 font-normal">{{ t("cut.segments.length") }}</th>
					<th scope="col" class="py-1 font-normal">
						<span class="sr-only">{{ t("cut.segments.actions") }}</span>
					</th>
				</tr>
			</thead>
			<tbody>
				<template v-for="(segment, index) in segments" :key="segment.id">
					<tr
						class="cursor-pointer border-t border-border align-middle hover:bg-surface-sunken/60"
						:class="{ 'bg-accent-surface/60': segment.id === selectedId }"
						:aria-selected="segment.id === selectedId"
						@click="emit('update:selectedId', segment.id)"
					>
						<td class="py-1 pr-2 font-mono text-muted tabular-nums">{{ index + 1 }}</td>
						<td class="py-1 pr-2" @click.stop>
							<CutTimeField
								:seconds="segment.start"
								:file-id="fileId"
								:label="t('cut.segments.startOf', { n: index + 1 })"
								:invalid="problems[segment.id] !== undefined"
								@commit="setTime(segment, 'start', $event)"
							/>
						</td>
						<td class="py-1 pr-2" @click.stop>
							<CutTimeField
								:seconds="segment.end"
								:file-id="fileId"
								:label="t('cut.segments.endOf', { n: index + 1 })"
								:invalid="problems[segment.id] !== undefined"
								@commit="setTime(segment, 'end', $event)"
							/>
						</td>
						<td class="py-1 pr-2 font-mono text-muted tabular-nums">{{ formatTimecode(segment.end - segment.start) }}</td>
						<td class="py-1">
							<div class="flex items-center justify-end gap-0.5">
								<ShellIconButton :label="t('cut.segments.goTo', { n: index + 1 })" @click="emit('seek', segment.start)">
									<IconLocate class="size-4" />
								</ShellIconButton>
								<ShellIconButton :label="t('cut.segments.remove', { n: index + 1 })" @click="emit('remove', segment.id)">
									<IconTrash class="size-4" />
								</ShellIconButton>
							</div>
						</td>
					</tr>
					<tr v-if="problems[segment.id]" class="text-xs text-danger">
						<td />
						<td colspan="4" class="pb-1">{{ t(`cut.segments.errors.${problems[segment.id]}`) }}</td>
					</tr>
				</template>
			</tbody>
		</table>
	</div>
</template>
