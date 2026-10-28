<script setup lang="ts">
import type { AppError } from "~/core/libs/errors";
import type { PickedFile } from "~/core/libs/filePicker";

/** Files screen placeholder; its "Open files…" button already goes through the file-picker abstraction. */
const { t } = useI18n();
const picker = useFilePicker();
const picked = ref<PickedFile[]>();
const error = ref<AppError>();

async function openFiles() {
	error.value = undefined;
	try {
		picked.value = await picker.pickFiles({ extensions: ["mkv", "mp4", "webm", "m2ts", "ts", "mov", "avi"] });
	} catch (cause) {
		error.value = toAppError(cause);
	}
}
</script>

<template>
	<PlaceholderPage name="files">
		<div class="mt-6 flex flex-col items-start gap-3">
			<button
				type="button"
				class="rounded-md bg-accent px-3 py-1.5 text-sm text-on-accent focus-visible:outline-2 focus-visible:outline-accent"
				@click="openFiles"
			>
				{{ t("pages.files.open") }}
			</button>
			<p v-if="picked" class="text-sm text-muted">
				{{ picked.length > 0 ? t("pages.files.picked", { names: picked.map((file) => file.name).join(", ") }) : t("pages.files.nothing") }}
			</p>
			<ErrorPanel v-if="error" :error="error" class="w-full max-w-prose" />
		</div>
	</PlaceholderPage>
</template>
