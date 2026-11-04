<script setup lang="ts">
import { entriesForPicked } from "~/core/libs/pickedFiles";
import { type AppError, toAppError } from "~/core/libs/errors";
import { useRpcClient } from "~/core/libs/rpc";
import { useMergeCheck } from "~/features/merge/composables/useMergeCheck";
import { jobLink } from "~/core/libs/navigation";
import { canStart, type MergeForm, toOperation, toggleDrop } from "~/features/merge/lib/merge";
import { FilesService, type Entry } from "~/generated/muxmultitool/files/v1/files_pb";
import { JobsService } from "~/generated/muxmultitool/jobs/v1/jobs_pb";

/**
 * Merge screen (sketch 06): the files of the selection in output order, the compatibility check that
 * names what differs, the command, and a Merge button that submits a job and links to its progress.
 */
const { t } = useI18n();
const selection = useFileSelectionStore();
const picker = useFilePicker();
const filesClient = useRpcClient(FilesService);
const jobsClient = useRpcClient(JobsService);

const dropStreams = ref<number[]>([]);
const chapters = ref(true);
const output = ref("");
const form = computed<MergeForm>(() => ({
	inputs: selection.ids,
	dropStreams: dropStreams.value,
	chapters: chapters.value,
	output: output.value
}));
const { check, checking, error: checkError } = useMergeCheck(form);

// The streams are numbered by the first file, so a different first file starts over
watch(
	() => selection.ids[0],
	() => {
		dropStreams.value = [];
	}
);

const names = computed(() => selection.files.map((file) => file.name));
const startable = computed(() => canStart(check.value, selection.count) && !checking.value);
const problemCount = computed(() => check.value?.problems.length ?? 0);

const error = ref<AppError>();
const submitting = ref(false);
const submitted = ref<{ id: string; name: string }>();

async function addFiles() {
	error.value = undefined;
	try {
		const picked = await picker.pickFiles({ extensions: ["mkv", "mp4", "webm", "m2ts", "ts", "mov", "avi"], multiple: true });
		selection.add(await entriesForPicked(filesClient, picker.mode, picked));
	} catch (cause) {
		error.value = toAppError(cause);
	}
}

async function merge() {
	error.value = undefined;
	submitted.value = undefined;
	submitting.value = true;
	try {
		const response = await jobsClient.submitJob({ operation: { case: "merge", value: toOperation(form.value) } });
		if (response.job) {
			submitted.value = { id: response.job.id, name: response.job.name };
		}
	} catch (cause) {
		error.value = toAppError(cause);
	} finally {
		submitting.value = false;
	}
}

const reorder = (files: Entry[]) => selection.set(files);
</script>

<template>
	<section class="flex max-w-5xl flex-col gap-6 p-6">
		<header>
			<h2 class="text-xl font-bold">{{ t("nav.merge") }}</h2>
			<p class="mt-1 max-w-prose text-sm text-muted">{{ t("merge.description") }}</p>
		</header>

		<MergeOrderList
			:files="selection.files"
			:parts="check?.parts ?? []"
			@reorder="reorder"
			@remove="selection.remove([$event])"
			@add="addFiles"
		/>

		<p v-if="selection.count === 1" class="text-sm text-muted">{{ t("merge.needTwo") }}</p>
		<ErrorPanel v-if="checkError" :error="checkError" />
		<MergeCompatibility
			v-else-if="check"
			:check="check"
			:names="names"
			:dropped="dropStreams"
			@toggle-drop="dropStreams = toggleDrop(dropStreams, $event)"
		/>

		<section v-if="selection.count >= 2" class="flex flex-col gap-3" :aria-label="t('merge.output.title')">
			<h3 class="font-bold">{{ t("merge.output.title") }}</h3>
			<label class="flex flex-col gap-1 text-sm">
				{{ t("merge.output.file") }}
				<input
					v-model="output"
					type="text"
					class="max-w-xl rounded-md border border-border bg-surface px-2 py-1.5"
					:placeholder="t('merge.output.placeholder')"
				/>
				<span class="text-xs text-muted">{{ t("merge.output.hint") }}</span>
			</label>
			<label class="flex items-center gap-2 text-sm">
				<input v-model="chapters" type="checkbox" class="accent-accent" />
				{{ t("merge.output.chapters") }}
			</label>
			<div v-if="check?.command">
				<h4 class="mb-1 text-sm font-bold">{{ t("merge.command.title") }}</h4>
				<pre class="overflow-x-auto rounded-md border border-border bg-surface-sunken p-3 text-xs whitespace-pre-wrap">{{
					check.command
				}}</pre>
			</div>
			<div class="flex flex-wrap items-center gap-3">
				<button
					type="button"
					class="rounded-md bg-accent px-4 py-1.5 text-sm text-on-accent focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-50"
					:disabled="!startable || submitting"
					@click="merge"
				>
					{{ t("merge.start") }}
				</button>
				<span v-if="checking" class="text-sm text-muted">{{ t("merge.checking") }}</span>
				<span v-else-if="problemCount > 0" class="text-sm text-danger">{{ t("merge.resolve", { n: problemCount }, problemCount) }}</span>
			</div>
		</section>

		<ErrorPanel v-if="error" :error="error" />
		<p v-if="submitted" class="text-sm" role="status">
			{{ t("merge.submitted", { name: submitted.name }) }}
			<RouterLink :to="jobLink(submitted.id)" class="text-accent underline">{{ t("merge.viewJob") }}</RouterLink>
		</p>
	</section>
</template>
