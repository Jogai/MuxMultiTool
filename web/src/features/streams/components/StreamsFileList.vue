<script setup lang="ts">
import IconX from "~icons/lucide/x";

import { formatGb } from "~/core/libs/reportFormat";

/** One file of the selection as the per-file list shows it. */
export interface FileSummary {
	id: string;
	name: string;
	/** Streams kept of the total; undefined until the preview is in. */
	kept?: number;
	total?: number;
	/** Bytes the output loses against the input (copy only). */
	droppedBytes?: number;
	/** The output file name, or the number of files when extracting. */
	output?: string;
	state: "loading" | "ready" | "blocked" | "error";
	/** Problems and warnings, counted for the badge. */
	problems: number;
}

/**
 * The per-file list of the sketch: every file of the selection with its result, as tabs that pick the
 * file whose table and command are shown. A file can be dropped from the selection here. The host
 * renders it inside a `RekaTabsRoot` keyed by file id, with the active file's panel as `RekaTabsContent`,
 * so the tabs get the arrow keys and the panel link from Reka.
 */
const { files, activeId } = defineProps<{ files: readonly FileSummary[]; activeId?: string }>();
const emit = defineEmits<{ remove: [id: string]; add: [] }>();
const { t } = useI18n();

const stateText = (file: FileSummary) => t(`streams.files.state.${file.state}`);
</script>

<template>
	<section class="flex flex-col gap-2" :aria-label="t('streams.files.title')">
		<h3 class="font-bold">{{ t("streams.files.title", { n: files.length }, files.length) }}</h3>
		<RekaTabsList as-child>
			<ul
				class="flex flex-col divide-y divide-border rounded-md border border-border"
				:aria-label="t('streams.files.title', { n: files.length }, files.length)"
			>
				<li
					v-for="file in files"
					:key="file.id"
					class="flex items-center gap-2 text-sm"
					:class="{ 'bg-accent-surface': file.id === activeId }"
				>
					<RekaTabsTrigger
						:value="file.id"
						class="flex min-w-0 flex-1 flex-wrap items-center gap-x-3 gap-y-0.5 px-3 py-1.5 text-left focus-visible:outline-2 focus-visible:outline-accent"
					>
						<span class="min-w-0 flex-1 basis-48 truncate font-bold" :title="file.name">{{ file.name }}</span>
						<span v-if="file.kept !== undefined" class="tabular-nums whitespace-nowrap">
							{{ t("streams.files.streams", { kept: file.kept, total: file.total }) }}
						</span>
						<span v-if="file.droppedBytes && formatGb(file.droppedBytes) !== '0.00'" class="tabular-nums text-muted">
							−{{ formatGb(file.droppedBytes) }} GB
						</span>
						<span v-if="file.output" class="min-w-0 truncate font-mono text-xs text-muted" :title="file.output">→ {{ file.output }}</span>
						<span
							class="whitespace-nowrap"
							:class="{
								'text-muted': file.state === 'loading',
								'text-success': file.state === 'ready' && file.problems === 0,
								'text-warning': file.state === 'ready' && file.problems > 0,
								'text-danger': file.state === 'blocked' || file.state === 'error'
							}"
						>
							{{ stateText(file) }}
							<template v-if="file.state === 'ready' && file.problems > 0"
								>({{ t("streams.files.warnings", { n: file.problems }, file.problems) }})</template
							>
						</span>
					</RekaTabsTrigger>
					<button
						type="button"
						class="mr-1 rounded-md p-1.5 hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent"
						:aria-label="t('streams.files.remove', { name: file.name })"
						@click="emit('remove', file.id)"
					>
						<IconX class="size-4" aria-hidden="true" />
					</button>
				</li>
			</ul>
		</RekaTabsList>
		<button
			type="button"
			class="self-start rounded-md border border-border px-3 py-1.5 text-sm hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent"
			@click="emit('add')"
		>
			{{ t("streams.files.add") }}
		</button>
	</section>
</template>
