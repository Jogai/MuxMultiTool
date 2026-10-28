<script setup lang="ts">
import IconChevronsLeft from "~icons/lucide/chevrons-left";
import IconChevronsRight from "~icons/lucide/chevrons-right";
import IconPause from "~icons/lucide/pause";
import IconPlay from "~icons/lucide/play";
import IconRepeat from "~icons/lucide/repeat";
import IconSkipBack from "~icons/lucide/skip-back";
import IconSkipForward from "~icons/lucide/skip-forward";
import IconStepBack from "~icons/lucide/step-back";
import IconStepForward from "~icons/lucide/step-forward";
import IconX from "~icons/lucide/x";

import { formatTimecode } from "~/core/libs/format";

import type { PlayerRange } from "../lib/range";
import type { PlayerAction } from "../lib/shortcuts";
import { frameIndex, parseTimecode } from "../lib/timecode";

/**
 * The transport row from the gui/04-cut sketch: keyframe and frame stepping around play/pause, the
 * timecode (editable: type a time and press Enter), the seek bar, and the range controls.
 */
const {
	paused,
	currentTime,
	duration,
	frameRate,
	hasKeyframes,
	range,
	loop,
	playingRange = false
} = defineProps<{
	paused: boolean;
	currentTime: number;
	duration: number;
	frameRate?: number;
	hasKeyframes: boolean;
	range: PlayerRange;
	loop: boolean;
	playingRange?: boolean;
}>();
const emit = defineEmits<{ action: [action: PlayerAction]; seek: [seconds: number]; clearRange: [] }>();
const { t } = useI18n();

const timecode = computed(() => formatTimecode(currentTime));
const frame = computed(() => (frameRate ? frameIndex(currentTime, frameRate) : undefined));
const hasRange = computed(() => range.start !== undefined || range.end !== undefined);

// The timecode field: shows the playhead until focused, then holds what is typed
const draft = ref<string>();
const invalid = ref(false);
function commitDraft(event: Event): void {
	const text = draft.value ?? "";
	const seconds = parseTimecode(text, frameRate);
	if (seconds === undefined) {
		invalid.value = true;
		return;
	}
	invalid.value = false;
	emit("seek", seconds);
	(event.target as HTMLInputElement).blur();
}
function endEdit(): void {
	draft.value = undefined;
	invalid.value = false;
}

const button =
	"flex h-8 items-center gap-1.5 rounded-md border border-border px-2 text-sm hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent aria-disabled:cursor-not-allowed aria-disabled:opacity-50 aria-pressed:border-accent aria-pressed:bg-accent-surface aria-pressed:text-accent";
</script>

<template>
	<div class="flex flex-col gap-2">
		<input
			type="range"
			class="w-full accent-(--mmt-accent)"
			min="0"
			:max="duration || 0"
			step="0.001"
			:value="Math.min(currentTime, duration || currentTime)"
			:aria-label="t('player.transport.seekBar')"
			:aria-valuetext="timecode"
			@input="emit('seek', Number(($event.target as HTMLInputElement).value))"
		/>
		<div class="flex flex-wrap items-center gap-2">
			<div class="flex items-center gap-0.5" role="group" :aria-label="t('player.transport.group')">
				<ShellIconButton
					:label="hasKeyframes ? t('player.transport.previousKeyframe') : t('player.transport.noKeyframes')"
					:disabled="!hasKeyframes"
					@click="emit('action', 'keyframeBack')"
				>
					<IconChevronsLeft />
				</ShellIconButton>
				<ShellIconButton :label="t('player.transport.previousFrame')" @click="emit('action', 'frameBack')">
					<IconStepBack />
				</ShellIconButton>
				<ShellIconButton :label="paused ? t('player.transport.play') : t('player.transport.pause')" @click="emit('action', 'togglePlay')">
					<IconPlay v-if="paused" />
					<IconPause v-else />
				</ShellIconButton>
				<ShellIconButton :label="t('player.transport.nextFrame')" @click="emit('action', 'frameForward')">
					<IconStepForward />
				</ShellIconButton>
				<ShellIconButton
					:label="hasKeyframes ? t('player.transport.nextKeyframe') : t('player.transport.noKeyframes')"
					:disabled="!hasKeyframes"
					@click="emit('action', 'keyframeForward')"
				>
					<IconChevronsRight />
				</ShellIconButton>
			</div>

			<label class="flex items-baseline gap-2">
				<span class="sr-only">{{ t("player.transport.timecode") }}</span>
				<input
					class="w-[14ch] rounded-md border bg-surface-sunken px-1.5 py-1 font-mono text-base tabular-nums focus-visible:outline-2 focus-visible:outline-accent"
					:class="invalid ? 'border-danger' : 'border-border'"
					:value="draft ?? timecode"
					:title="t('player.transport.timecode')"
					:aria-invalid="invalid || undefined"
					spellcheck="false"
					@focus="draft = timecode"
					@input="draft = ($event.target as HTMLInputElement).value"
					@keydown.enter.prevent="commitDraft"
					@keydown.esc.prevent="($event.target as HTMLInputElement).blur()"
					@blur="endEdit"
				/>
				<span v-if="frame !== undefined" class="font-mono text-xs text-muted tabular-nums">
					{{ t("player.transport.frame", { n: frame }) }}
				</span>
			</label>

			<div class="ml-auto flex flex-wrap items-center gap-1">
				<button type="button" :class="button" @click="emit('action', 'setRangeStart')">
					<span aria-hidden="true">[</span>
					{{ t("player.range.setStart") }}
				</button>
				<button type="button" :class="button" @click="emit('action', 'setRangeEnd')">
					{{ t("player.range.setEnd") }}
					<span aria-hidden="true">]</span>
				</button>
				<span class="font-mono text-xs text-muted tabular-nums">
					{{
						hasRange
							? t("player.range.label", {
									start: range.start === undefined ? "…" : formatTimecode(range.start),
									end: range.end === undefined ? "…" : formatTimecode(range.end)
								})
							: t("player.range.none")
					}}
				</span>
				<button
					v-if="hasRange"
					type="button"
					class="flex size-8 items-center justify-center rounded-md text-muted hover:bg-surface-sunken hover:text-text focus-visible:outline-2 focus-visible:outline-accent"
					:aria-label="t('player.range.clear')"
					:title="t('player.range.clear')"
					@click="emit('clearRange')"
				>
					<IconX class="size-4" />
				</button>
				<button
					type="button"
					:class="button"
					:aria-disabled="!hasRange || undefined"
					:aria-pressed="playingRange || undefined"
					@click="hasRange && emit('action', 'playRange')"
				>
					<IconSkipBack class="size-4" aria-hidden="true" />
					{{ t("player.range.playRange") }}
					<IconSkipForward class="size-4" aria-hidden="true" />
				</button>
				<button type="button" :class="button" :aria-pressed="loop" @click="emit('action', 'toggleLoop')">
					<IconRepeat class="size-4" aria-hidden="true" />
					{{ t("player.range.loop") }}
				</button>
			</div>
		</div>
	</div>
</template>
