<script setup lang="ts">
import { formatBuildInfo, readBuildInfo } from "~/core/libs/buildInfo";
import { LICENSE_ID, LICENSE_URL, REPOSITORY_URL, openExternal } from "~/core/libs/externalLink";

/** About screen from the ☰ menu: project, version, license and the bundled ffmpeg (gui sketch 10). */
const open = defineModel<boolean>("open", { required: true });
const { t } = useI18n();
const mode = appMode();
const build = readBuildInfo(__APP_VERSION__);
const ffmpeg = useFfmpegInfo();
const clipboard = useClipboard();

watch(open, (isOpen) => {
	if (isOpen && ffmpeg.state.value !== "ok") void ffmpeg.load();
});

function onLink(event: MouseEvent, url: string) {
	if (openExternal(url)) event.preventDefault();
}

const linkClass = "text-accent underline focus-visible:outline-2 focus-visible:outline-accent";
</script>

<template>
	<RekaDialogRoot v-model:open="open">
		<RekaDialogPortal>
			<RekaDialogOverlay class="fixed inset-0 bg-overlay" />
			<RekaDialogContent
				class="fixed top-1/2 left-1/2 w-[min(28rem,calc(100vw-2rem))] -translate-1/2 rounded-lg border border-border bg-surface p-5 shadow-lg"
			>
				<RekaDialogTitle class="text-lg font-bold">{{ t("dialogs.about.title") }}</RekaDialogTitle>
				<RekaDialogDescription class="mt-1 text-sm text-muted">{{ t("app.tagline") }}</RekaDialogDescription>

				<dl class="mt-4 grid grid-cols-[6rem_1fr] gap-x-3 gap-y-2 text-sm">
					<dt class="text-muted">{{ t("dialogs.about.version") }}</dt>
					<dd>
						{{ build.version }} · {{ t(`statusBar.mode.${mode}`) }}
						<span v-if="build.commit" class="block text-xs text-muted">
							{{ t("dialogs.about.commit", { commit: build.commit }) }}
							<template v-if="build.buildTime"> · {{ t("dialogs.about.builtAt", { time: build.buildTime }) }}</template>
						</span>
					</dd>

					<dt class="text-muted">{{ t("dialogs.about.project") }}</dt>
					<dd>
						<a :href="REPOSITORY_URL" target="_blank" rel="noopener noreferrer" :class="linkClass" @click="onLink($event, REPOSITORY_URL)">
							{{ REPOSITORY_URL.replace("https://", "") }} ↗
						</a>
					</dd>

					<dt class="text-muted">{{ t("dialogs.about.license") }}</dt>
					<dd>
						{{ LICENSE_ID }}
						<a
							:href="LICENSE_URL"
							target="_blank"
							rel="noopener noreferrer"
							:class="[linkClass, 'block']"
							@click="onLink($event, LICENSE_URL)"
						>
							{{ LICENSE_URL.replace("https://", "") }} ↗
						</a>
					</dd>

					<dt class="text-muted">ffmpeg</dt>
					<dd>
						<span v-if="ffmpeg.state.value === 'ok'">{{ t("dialogs.about.ffmpegBuild", { version: ffmpeg.version.value }) }}</span>
						<span v-else-if="ffmpeg.state.value === 'unavailable'" class="text-danger">
							{{ t("dialogs.about.ffmpegUnavailable") }}
						</span>
						<span v-else-if="ffmpeg.state.value === 'error'" class="text-danger">{{ t("dialogs.about.ffmpegError") }}</span>
						<span v-else class="text-muted">{{ t("dialogs.about.ffmpegLoading") }}</span>
					</dd>
				</dl>

				<div class="mt-5 flex items-center justify-end gap-2">
					<span v-if="clipboard.copied.value" role="status" class="mr-auto text-xs text-muted">{{ t("dialogs.about.copied") }}</span>
					<button
						type="button"
						class="rounded-md border border-border px-3 py-1.5 text-sm hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent"
						@click="clipboard.copy(formatBuildInfo(build, ffmpeg.version.value))"
					>
						{{ t("dialogs.about.copy") }}
					</button>
					<RekaDialogClose
						class="rounded-md bg-accent px-3 py-1.5 text-sm text-on-accent focus-visible:outline-2 focus-visible:outline-accent"
					>
						{{ t("dialogs.close") }}
					</RekaDialogClose>
				</div>
			</RekaDialogContent>
		</RekaDialogPortal>
	</RekaDialogRoot>
</template>
