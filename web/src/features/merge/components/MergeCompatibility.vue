<script setup lang="ts">
import { mismatchesOfPart } from "../lib/merge";
import { streamTypeName } from "~/core/libs/reportFormat";
import { type CheckMergeResponse, MergeParameter } from "~/generated/muxmultitool/merge/v1/merge_pb";

/**
 * The compatibility table of the sketch: a row per stream of the first file, a column per part, and
 * the result naming what differs. A cell that differs from the first file is marked, and a stream
 * that differs can be left out, which is the lossless fix. Problems the table cannot place, such as
 * a different stream layout, are listed under it.
 */
const { check, names, dropped } = defineProps<{ check: CheckMergeResponse; names: readonly string[]; dropped: readonly number[] }>();
const emit = defineEmits<{ toggleDrop: [index: number] }>();
const { t } = useI18n();

const layoutProblems = computed(() => check.problems.filter((problem) => problem.parameter === MergeParameter.STREAM_LAYOUT));
</script>

<template>
	<section :aria-label="t('merge.check.title')">
		<h3 class="mb-2 font-bold">{{ t("merge.check.title") }}</h3>
		<div class="overflow-x-auto">
			<table class="w-full border-collapse text-sm">
				<thead class="text-left text-xs text-muted uppercase">
					<tr class="border-b border-border">
						<th scope="col" class="py-2 pr-3">{{ t("merge.check.stream") }}</th>
						<th v-for="(name, part) in names" :key="part" scope="col" class="py-2 pr-3 normal-case">{{ name }}</th>
						<th scope="col" class="py-2">{{ t("merge.check.result") }}</th>
					</tr>
				</thead>
				<tbody>
					<tr v-for="stream in check.streams" :key="stream.index" class="border-b border-border align-top">
						<th scope="row" class="py-1.5 pr-3 text-left font-normal whitespace-nowrap">
							#{{ stream.index }} {{ streamTypeName(stream.type) }}
						</th>
						<td
							v-for="(cell, part) in stream.cells"
							:key="part"
							class="py-1.5 pr-3"
							:class="mismatchesOfPart(stream.mismatches, part).length > 0 ? 'font-bold text-danger' : ''"
						>
							{{ cell === "" ? t("merge.check.missing") : cell }}
						</td>
						<td class="py-1.5">
							<template v-if="stream.identical">
								<span class="text-success">✓ {{ t("merge.check.identical") }}</span>
							</template>
							<template v-else>
								<ul>
									<li v-for="mismatch in stream.mismatches" :key="`${mismatch.part}-${mismatch.parameter}`" class="text-danger">
										<span aria-hidden="true">{{ stream.dropped ? "–" : "✕" }}</span> {{ mismatch.message }}
									</li>
								</ul>
								<label class="mt-1 flex items-center gap-2">
									<input type="checkbox" class="accent-accent" :checked="stream.dropped" @change="emit('toggleDrop', stream.index)" />
									{{ t("merge.check.leaveOut", { index: stream.index }) }}
								</label>
							</template>
						</td>
					</tr>
				</tbody>
			</table>
		</div>
		<ul v-if="layoutProblems.length > 0" class="mt-2 text-sm text-danger">
			<li v-for="problem in layoutProblems" :key="problem.part">✕ {{ problem.message }}</li>
		</ul>
		<p v-if="dropped.length > 0" class="mt-2 text-sm text-muted">
			{{ t("merge.check.droppedNote", { n: dropped.length }, dropped.length) }}
		</p>
	</section>
</template>
