<script setup lang="ts">
import IconArrowDown from "~icons/lucide/arrow-down";
import IconArrowUp from "~icons/lucide/arrow-up";
import IconX from "~icons/lucide/x";

import {
	addRule,
	isRuleComplete,
	move,
	QUICK_RULES,
	RULE_STREAM_TYPES,
	type RuleDraft,
	type RuleMatchKind,
	ruleName
} from "../lib/streams";
import { streamTypeName } from "~/core/libs/reportFormat";
import { StreamType } from "~/generated/muxmultitool/files/v1/files_pb";

/**
 * The rules of the sketch: they apply to every file of the selection. A list with remove and reorder
 * (the order never changes the result, but a tidy list is easier to read), the "add rule" form, the
 * quick chips for the MVP's common cases, and the untagged-language switch of the backend's rules.
 * "Video only" is a mode rather than a rule, because a rule always needs something to match.
 */
const { fileCount } = defineProps<{ fileCount: number }>();
const rules = defineModel<readonly RuleDraft[]>("rules", { required: true });
const untagged = defineModel<"keep" | "drop">("untagged", { required: true });
const videoOnly = defineModel<boolean>("videoOnly", { required: true });
const { t } = useI18n();

const draft = ref<RuleDraft>({ action: "drop", streamType: StreamType.AUDIO, match: "language", value: "" });
const valueField = useTemplateRef<HTMLInputElement>("valueField");
const draftComplete = computed(() => isRuleComplete(draft.value));

function add() {
	if (!draftComplete.value) {
		return;
	}
	rules.value = addRule(rules.value, draft.value);
	draft.value = { ...draft.value, value: "" };
	valueField.value?.focus();
}

function addQuick(quick: readonly RuleDraft[]) {
	let next = rules.value;
	for (const rule of quick) {
		next = addRule(next, rule);
	}
	rules.value = next;
}

function remove(index: number) {
	rules.value = rules.value.filter((_, i) => i !== index);
}

/** Puts the caret in the value field, for the `n` shortcut. */
function focusNew() {
	valueField.value?.focus();
}
defineExpose({ focusNew });

const typeName = (type: StreamType) => (type === StreamType.UNSPECIFIED ? t("streams.rules.anyType") : streamTypeName(type));
const matchKinds: readonly RuleMatchKind[] = ["language", "codec", "streamIndex"];
const placeholder = computed(() => t(`streams.rules.placeholder.${draft.value.match}`));

const select = "rounded-md border border-border bg-surface px-2 py-1 text-sm";
const chip =
	"rounded-full border border-border px-3 py-0.5 text-sm hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent aria-pressed:border-accent aria-pressed:bg-accent aria-pressed:text-on-accent";
const iconButton =
	"rounded-md p-1 hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-40 disabled:hover:bg-transparent";
</script>

<template>
	<section class="flex flex-col gap-3" :aria-label="t('streams.rules.title')">
		<h3 class="font-bold">
			{{ t("streams.rules.title") }}
			<span class="font-normal text-muted">· {{ t("streams.rules.applyTo", { n: fileCount }, fileCount) }}</span>
		</h3>

		<ul v-if="rules.length > 0" class="flex flex-col gap-1" :class="{ 'opacity-50': videoOnly }">
			<li v-for="(rule, index) in rules" :key="`${index}-${ruleName(rule, typeName)}`" class="flex items-center gap-2 text-sm">
				<span class="font-mono" :class="{ 'line-through': !isRuleComplete(rule) }">{{ ruleName(rule, typeName) }}</span>
				<span class="ml-auto flex">
					<button
						type="button"
						:class="iconButton"
						:disabled="index === 0"
						:aria-label="t('streams.rules.up')"
						@click="rules = move(rules, index, index - 1)"
					>
						<IconArrowUp class="size-4" aria-hidden="true" />
					</button>
					<button
						type="button"
						:class="iconButton"
						:disabled="index === rules.length - 1"
						:aria-label="t('streams.rules.down')"
						@click="rules = move(rules, index, index + 1)"
					>
						<IconArrowDown class="size-4" aria-hidden="true" />
					</button>
					<button
						type="button"
						:class="iconButton"
						:aria-label="t('streams.rules.remove', { rule: ruleName(rule, typeName) })"
						@click="remove(index)"
					>
						<IconX class="size-4" aria-hidden="true" />
					</button>
				</span>
			</li>
		</ul>
		<p v-else class="text-sm text-muted">{{ t("streams.rules.empty") }}</p>
		<p v-if="videoOnly" class="text-sm text-muted">{{ t("streams.rules.videoOnlyNote") }}</p>

		<form class="flex flex-wrap items-center gap-2" @submit.prevent="add">
			<span class="text-sm">{{ t("streams.rules.add") }}</span>
			<select v-model="draft.action" :class="select" :aria-label="t('streams.rules.action')">
				<option value="drop">{{ t("streams.rules.drop") }}</option>
				<option value="keep">{{ t("streams.rules.keep") }}</option>
			</select>
			<select v-model="draft.streamType" :class="select" :aria-label="t('streams.rules.type')">
				<option v-for="type in RULE_STREAM_TYPES" :key="type" :value="type">{{ typeName(type) }}</option>
			</select>
			<span class="text-sm text-muted">{{ t("streams.rules.where") }}</span>
			<select v-model="draft.match" :class="select" :aria-label="t('streams.rules.match')">
				<option v-for="kind in matchKinds" :key="kind" :value="kind">{{ t(`streams.rules.matches.${kind}`) }}</option>
			</select>
			<span class="text-sm text-muted">=</span>
			<input
				ref="valueField"
				v-model="draft.value"
				type="text"
				class="w-28 rounded-md border border-border bg-surface px-2 py-1 font-mono text-sm"
				:placeholder="placeholder"
				:aria-label="t('streams.rules.value')"
			/>
			<button
				type="submit"
				class="rounded-md border border-border px-3 py-1 text-sm hover:bg-surface-sunken focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-50"
				:disabled="!draftComplete"
			>
				{{ t("streams.rules.addButton") }}
			</button>
		</form>

		<div class="flex flex-wrap items-center gap-2">
			<span class="text-sm text-muted">{{ t("streams.rules.quick") }}</span>
			<button type="button" :class="chip" :aria-pressed="videoOnly" @click="videoOnly = !videoOnly">
				{{ t("streams.rules.quickChips.videoOnly") }}
			</button>
			<button v-for="quick in QUICK_RULES" :key="quick.name" type="button" :class="chip" @click="addQuick(quick.rules)">
				{{ t(`streams.rules.quickChips.${quick.name}`) }}
			</button>
			<label class="ml-auto flex items-center gap-2 text-sm">
				{{ t("streams.rules.untagged") }}
				<select v-model="untagged" :class="select">
					<option value="keep">{{ t("streams.rules.untaggedKeep") }}</option>
					<option value="drop">{{ t("streams.rules.untaggedDrop") }}</option>
				</select>
			</label>
		</div>
	</section>
</template>
