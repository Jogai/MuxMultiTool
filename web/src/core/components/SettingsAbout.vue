<script setup lang="ts">
import { formatBuildInfo, readBuildInfo } from "~/core/libs/buildInfo";
import { LICENSE_ID, LICENSE_URL, REPOSITORY_URL, openExternal } from "~/core/libs/externalLink";

/** The About section of Settings: project, version, license and the ffmpeg build (gui sketch 10, once a dialog). */
const { t } = useI18n();
const mode = appMode();
const build = readBuildInfo(__APP_VERSION__);
const ffmpeg = useFfmpegInfo();
const clipboard = useClipboard();

onMounted(() => void ffmpeg.load());

function onLink(event: MouseEvent, url: string) {
	if (openExternal(url)) event.preventDefault();
}

const linkClass = "text-accent underline focus-visible:outline-2 focus-visible:outline-accent";
</script>

<template>
	<div class="max-w-prose">
		<p class="text-sm text-muted">{{ t("app.tagline") }}</p>

		<dl class="mt-4 grid grid-cols-[6rem_1fr] gap-x-3 gap-y-2 text-sm">
			<dt class="text-muted">{{ t("pages.settings.about.version") }}</dt>
			<dd>
				{{ build.version }} · {{ t(`statusBar.mode.${mode}`) }}
				<span v-if="build.commit" class="block text-xs text-muted">
					{{ t("pages.settings.about.commit", { commit: build.commit }) }}
					<template v-if="build.buildTime"> · {{ t("pages.settings.about.builtAt", { time: build.buildTime }) }}</template>
				</span>
			</dd>

			<dt class="text-muted">{{ t("pages.settings.about.project") }}</dt>
			<dd>
				<a :href="REPOSITORY_URL" target="_blank" rel="noopener noreferrer" :class="linkClass" @click="onLink($event, REPOSITORY_URL)">
					{{ REPOSITORY_URL.replace("https://", "") }} ↗
				</a>
			</dd>

			<dt class="text-muted">{{ t("pages.settings.about.license") }}</dt>
			<dd>
				{{ LICENSE_ID }}
				<a :href="LICENSE_URL" target="_blank" rel="noopener noreferrer" :class="[linkClass, 'block']" @click="onLink($event, LICENSE_URL)">
					{{ LICENSE_URL.replace("https://", "") }} ↗
				</a>
			</dd>

			<dt class="text-muted">ffmpeg</dt>
			<dd>
				<span v-if="ffmpeg.state.value === 'ok'">{{ t("pages.settings.about.ffmpegBuild", { version: ffmpeg.version.value }) }}</span>
				<span v-else-if="ffmpeg.state.value === 'unavailable'" class="text-danger">{{ t("pages.settings.about.ffmpegUnavailable") }}</span>
				<span v-else-if="ffmpeg.state.value === 'error'" class="text-danger">{{ t("pages.settings.about.ffmpegError") }}</span>
				<span v-else class="text-muted">{{ t("pages.settings.about.ffmpegLoading") }}</span>
			</dd>
		</dl>

		<div class="mt-4 flex items-center gap-3">
			<button
				type="button"
				class="rounded-md border border-border px-3 py-1.5 text-sm hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent"
				@click="clipboard.copy(formatBuildInfo(build, ffmpeg.version.value))"
			>
				{{ t("pages.settings.about.copy") }}
			</button>
			<span v-if="clipboard.copied.value" role="status" class="text-xs text-muted">{{ t("pages.settings.about.copied") }}</span>
		</div>
	</div>
</template>
