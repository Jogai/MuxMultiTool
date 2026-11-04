<script setup lang="ts">
import IconArrowDown from "~icons/lucide/arrow-down";
import IconArrowUp from "~icons/lucide/arrow-up";
import IconGripVertical from "~icons/lucide/grip-vertical";
import IconX from "~icons/lucide/x";

import { move } from "~/core/libs/collections";
import { formatDuration } from "~/core/libs/reportFormat";
import type { Entry } from "~/generated/muxmultitool/files/v1/files_pb";
import type { MergePart } from "~/generated/muxmultitool/merge/v1/merge_pb";

/**
 * The parts in output order. They can be dragged, or moved with the buttons (the keyboard path); the
 * duration and the stream layout come from the compatibility check once it has run.
 */
const { files, parts } = defineProps<{ files: readonly Entry[]; parts: readonly MergePart[] }>();
const emit = defineEmits<{ reorder: [files: Entry[]]; remove: [id: string]; add: [] }>();
const { t } = useI18n();

const dragging = ref<number>();

function moveTo(from: number, to: number) {
	emit("reorder", move(files, from, to));
}

function drop(to: number) {
	if (dragging.value !== undefined) {
		moveTo(dragging.value, to);
	}
	dragging.value = undefined;
}

const partOf = (index: number): MergePart | undefined => (parts.length === files.length ? parts[index] : undefined);
const durationOf = (index: number) => {
	const ms = partOf(index)?.durationMs;
	return ms ? formatDuration(Number(ms) / 1000) : undefined;
};

const button =
	"rounded-md p-1.5 hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-40 disabled:hover:bg-transparent";
</script>

<template>
	<section :aria-label="t('merge.order.title')">
		<h3 class="mb-2 font-bold">{{ t("merge.order.title") }}</h3>
		<p v-if="files.length === 0" class="text-sm text-muted">{{ t("merge.order.empty") }}</p>
		<table v-else class="w-full border-collapse text-sm">
			<thead class="text-left text-xs text-muted uppercase">
				<tr class="border-b border-border">
					<th scope="col" class="w-8 py-2" />
					<th scope="col" class="w-8 py-2">#</th>
					<th scope="col" class="py-2">{{ t("merge.order.file") }}</th>
					<th scope="col" class="py-2">{{ t("merge.order.duration") }}</th>
					<th scope="col" class="py-2">{{ t("merge.order.layout") }}</th>
					<th scope="col" class="py-2 text-right">{{ t("merge.order.actions") }}</th>
				</tr>
			</thead>
			<tbody>
				<tr
					v-for="(file, index) in files"
					:key="file.id"
					class="border-b border-border"
					:class="{ 'bg-surface-sunken': dragging === index }"
					@dragover.prevent
					@drop.prevent="drop(index)"
				>
					<td class="py-1.5">
						<span
							class="inline-flex cursor-grab text-muted"
							draggable="true"
							:title="t('merge.order.drag')"
							@dragstart="dragging = index"
							@dragend="dragging = undefined"
						>
							<IconGripVertical aria-hidden="true" />
						</span>
					</td>
					<td class="py-1.5 text-muted">{{ index + 1 }}</td>
					<td class="py-1.5 break-all">{{ file.name }}</td>
					<td class="py-1.5 tabular-nums">{{ durationOf(index) ?? "–" }}</td>
					<td class="py-1.5 text-muted">{{ partOf(index)?.layout ?? "–" }}</td>
					<td class="py-1.5 text-right whitespace-nowrap">
						<button
							type="button"
							:class="button"
							:disabled="index === 0"
							:aria-label="t('merge.order.up', { name: file.name })"
							@click="moveTo(index, index - 1)"
						>
							<IconArrowUp aria-hidden="true" />
						</button>
						<button
							type="button"
							:class="button"
							:disabled="index === files.length - 1"
							:aria-label="t('merge.order.down', { name: file.name })"
							@click="moveTo(index, index + 1)"
						>
							<IconArrowDown aria-hidden="true" />
						</button>
						<button
							type="button"
							:class="button"
							:aria-label="t('merge.order.remove', { name: file.name })"
							@click="emit('remove', file.id)"
						>
							<IconX aria-hidden="true" />
						</button>
					</td>
				</tr>
			</tbody>
		</table>
		<button
			type="button"
			class="mt-3 rounded-md border border-border px-3 py-1.5 text-sm hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent"
			@click="emit('add')"
		>
			{{ t("merge.order.add") }}
		</button>
	</section>
</template>
