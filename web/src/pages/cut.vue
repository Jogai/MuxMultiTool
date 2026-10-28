<script setup lang="ts">
import type { AppError } from "~/core/libs/errors";
import type { PlayerRange } from "~/features/player/lib/range";

/**
 * Cut screen: the player (TASK-31) over the file named by the `file` query (`/cut?file=<id>`); the
 * timeline and segment editor follow with TASK-32. The keyframes come from the cut service then.
 */
const { t } = useI18n();
const route = useRoute();
const router = useRouter();
const picker = useFilePicker();
const fileId = computed(() => (typeof route.query.file === "string" && route.query.file !== "" ? route.query.file : undefined));
const range = ref<PlayerRange>({});
const keyframes = ref<number[]>([]);
const error = ref<AppError>();

async function openFile() {
	error.value = undefined;
	try {
		const [file] = await picker.pickFiles({ extensions: ["mkv", "mp4", "webm", "m2ts", "ts", "mov", "avi"] });
		if (file) {
			await router.replace({ query: { ...route.query, file: file.path } });
		}
	} catch (cause) {
		error.value = toAppError(cause);
	}
}
</script>

<template>
	<section class="flex flex-col gap-4 p-6">
		<header class="flex flex-wrap items-center gap-3">
			<h2 class="text-xl font-bold">{{ t("nav.cut") }}</h2>
			<button
				type="button"
				class="rounded-md bg-accent px-3 py-1.5 text-sm text-on-accent focus-visible:outline-2 focus-visible:outline-accent"
				@click="openFile"
			>
				{{ t("pages.cut.open") }}
			</button>
		</header>
		<ErrorPanel v-if="error" :error="error" class="max-w-prose" />
		<PlayerView v-if="fileId" :key="fileId" v-model:range="range" :file-id="fileId" :keyframes="keyframes" class="max-w-5xl" />
		<template v-else>
			<p class="max-w-prose text-muted">{{ t("pages.cut.description") }}</p>
			<p class="text-sm text-muted">{{ t("pages.cut.empty") }}</p>
			<p class="text-sm text-muted">{{ t("pages.arrives", { task: t("pages.cut.task") }) }}</p>
		</template>
	</section>
</template>
