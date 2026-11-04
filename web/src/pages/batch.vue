<script setup lang="ts">
import { type AppError, toAppError } from "~/core/libs/errors";
import { useRpcClient } from "~/core/libs/rpc";
import { type BatchOperation, BatchService, type PlanBatchResponse } from "~/generated/muxmultitool/batch/v1/batch_pb";
import { FilesService } from "~/generated/muxmultitool/files/v1/files_pb";
import { PresetsService } from "~/generated/muxmultitool/presets/v1/presets_pb";
import { formProblems, planRows, progressRows, splitPatterns, toApplyPresetRequest, toBatchOperation } from "~/features/batch/lib/batch";
import { useDebouncedCall } from "~/features/images/composables/useDebouncedCall";
import { usePresets } from "~/features/presets/composables/usePresets";
import { batchTemplateOf } from "~/features/presets/lib/presets";

/**
 * The Batch screen (sketch 09): Input, Operation and Output top to bottom, with the names the first
 * matching files would get as the template is typed; then the dry run, which the server plans per
 * file (output name, resolved streams, warnings, command, or why the file is skipped), and "Queue N
 * jobs", after which every file's job is followed here. The form and the queued batch live in the
 * batch store, so a detour to the Jobs screen loses nothing.
 */
const { t } = useI18n();
const store = useBatchStore();
const { form, submission } = storeToRefs(store);
const jobs = useJobsStore();
const filesClient = useRpcClient(FilesService);
const batchClient = useRpcClient(BatchService);
const presetsClient = useRpcClient(PresetsService);
const presets = usePresets(presetsClient);
onMounted(() => void presets.load());

// The matching files, for the count and the live name preview
const findRequest = computed(() => {
	const patterns = splitPatterns(form.value.patterns);
	if (form.value.folder === "" || patterns.length === 0) {
		return undefined;
	}
	return { folderId: form.value.folder, patterns, recursive: form.value.recursive };
});
const matches = useDebouncedCall(findRequest, (request, signal) => filesClient.findFiles(request, { signal }), 300);
const matchingFiles = computed(() => matches.result.value?.files ?? []);
const folderName = computed(() => baseName(form.value.folderLabel));

const problems = computed(() => formProblems(form.value));
const templateProblem = computed(() => {
	const found = problems.value.find((problem) => typeof problem === "object");
	return found === undefined ? undefined : found.template;
});
const preset = computed(() => (form.value.source === "preset" ? presets.byId.value.get(form.value.presetId) : undefined));
const presetTemplate = computed(() =>
	form.value.source === "preset" ? (preset.value?.settings ? batchTemplateOf(preset.value.settings) : "") : undefined
);

/** The operation a dry run or a submit sends: the form's own, or the preset's as the server renders it. */
async function operation(): Promise<BatchOperation> {
	if (form.value.source === "preset") {
		const response = await presetsClient.applyPresetToBatch(toApplyPresetRequest(form.value));
		if (!response.operation) {
			throw new Error("the preset gave no operation");
		}
		return response.operation;
	}
	return toBatchOperation(form.value);
}

// The dry run, tied to the form it was made for
const plan = shallowRef<PlanBatchResponse>();
const planKey = ref("");
const planning = ref(false);
const planError = ref<AppError>();
const formKey = computed(() => JSON.stringify(form.value));
const stale = computed(() => plan.value !== undefined && planKey.value !== formKey.value);
const rows = computed(() => (plan.value ? planRows(plan.value.items) : []));

async function dryRun() {
	if (problems.value.length > 0 || planning.value) {
		return;
	}
	planning.value = true;
	planError.value = undefined;
	const key = formKey.value;
	try {
		const response = await batchClient.planBatch({ operation: await operation() });
		plan.value = response;
		planKey.value = key;
	} catch (cause) {
		plan.value = undefined;
		planError.value = toAppError(cause);
	} finally {
		planning.value = false;
	}
}

// The submit: one job per ready file, followed through the shared queue
const submitting = ref(false);
const submitError = ref<AppError>();
const startable = computed(() => plan.value !== undefined && !stale.value && plan.value.ready > 0 && !planning.value && !submitting.value);

async function queue() {
	if (!startable.value) {
		return;
	}
	submitting.value = true;
	submitError.value = undefined;
	try {
		const response = await batchClient.submitBatch({ operation: await operation() });
		submission.value = {
			jobs: response.jobs.map((job) => ({ id: job.id, name: job.name })),
			skipped: response.skipped
		};
		plan.value = undefined;
	} catch (cause) {
		submitError.value = toAppError(cause);
	} finally {
		submitting.value = false;
	}
}

const progress = computed(() => (submission.value ? progressRows(submission.value, jobs.byId) : []));

function another() {
	submission.value = undefined;
}

const problemText = (problem: (typeof problems.value)[number]) =>
	typeof problem === "string" ? t(`batch.output.problems.${problem}`) : undefined;
</script>

<template>
	<section class="flex flex-col gap-6 p-6">
		<header>
			<h2 class="text-xl font-bold">{{ t("nav.batch") }}</h2>
			<p class="mt-1 max-w-prose text-sm text-muted">{{ t("batch.description") }}</p>
		</header>

		<JobsConnectionNotice v-if="submission" />
		<BatchProgress v-if="submission" :rows="progress" :skipped="submission.skipped" @another="another" />

		<template v-else>
			<BatchInput
				v-model:folder="form.folder"
				v-model:folder-label="form.folderLabel"
				v-model:patterns="form.patterns"
				v-model:recursive="form.recursive"
				:matches="matches.result.value?.files"
				:checking="matches.pending.value"
				:error="matches.error.value"
			/>

			<BatchOperation
				v-model:source="form.source"
				v-model:preset-id="form.presetId"
				v-model:rules="form.rules"
				v-model:untagged="form.untagged"
				v-model:video-only="form.videoOnly"
				:presets="presets.presets.value"
			/>
			<ErrorPanel v-if="presets.error.value" :error="presets.error.value" />

			<BatchOutput
				v-model:output-folder="form.outputFolder"
				v-model:output-folder-label="form.outputFolderLabel"
				v-model:template="form.template"
				v-model:overwrite="form.overwrite"
				:preset-template="presetTemplate"
				:files="matchingFiles"
				:folder-name="folderName"
				:problem="templateProblem"
				:date="plan?.date"
			/>

			<div class="flex flex-wrap items-center gap-3">
				<button
					type="button"
					class="rounded-md border border-border px-4 py-1.5 text-sm hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-50"
					:disabled="problems.length > 0 || planning"
					@click="dryRun"
				>
					{{ planning ? t("batch.plan.running") : t("batch.plan.dryRun") }}
				</button>
				<button
					type="button"
					class="rounded-md bg-accent px-4 py-1.5 text-sm text-on-accent focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-50"
					:disabled="!startable"
					@click="queue"
				>
					{{ submitting ? t("batch.queueing") : t("batch.queue", { n: plan?.ready ?? 0 }, plan?.ready ?? 0) }}
				</button>
				<ul v-if="problems.length > 0" class="flex flex-wrap gap-x-3 text-sm text-muted">
					<template v-for="(problem, index) in problems" :key="index">
						<li v-if="problemText(problem)">{{ problemText(problem) }}</li>
					</template>
				</ul>
			</div>

			<ErrorPanel v-if="planError" :error="planError" />
			<ErrorPanel v-if="submitError" :error="submitError" />
			<BatchPlan v-if="plan" :rows="rows" :ready="plan.ready" :date="plan.date" :stale="stale" />
		</template>
	</section>
</template>
