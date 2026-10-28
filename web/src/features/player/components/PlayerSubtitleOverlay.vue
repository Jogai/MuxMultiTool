<script setup lang="ts">
import type { SubtitleCue } from "~/generated/muxmultitool/media/v1/media_pb";

/** The cues that show now, drawn over the bottom of the picture the way a player burns them in. */
const { cues } = defineProps<{ cues: readonly SubtitleCue[] }>();
</script>

<template>
	<div
		v-if="cues.length > 0"
		class="pointer-events-none absolute inset-x-[4%] bottom-[5%] flex flex-col items-center gap-1"
		aria-live="polite"
	>
		<p
			v-for="(cue, index) in cues"
			:key="`${cue.startSeconds}-${index}`"
			class="cue max-w-full rounded px-2 py-0.5 text-center leading-snug font-bold whitespace-pre-line"
		>
			{{ cue.text }}
		</p>
	</div>
</template>

<style scoped>
/* Subtitles are part of the picture: white on a dark box, as players burn them in, whatever the theme */
.cue {
	color: #fff;
	background: rgb(0 0 0 / 0.75);
	font-size: clamp(0.75rem, 2.4cqw, 1.5rem);
	text-shadow: 0 1px 2px rgb(0 0 0 / 0.8);
}
</style>
