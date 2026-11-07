<script setup lang="ts">
import { SBOM_KINDS, SBOM_URL, countByKind, filterSbom, sortSbom } from "~/core/libs/sbom";

/**
 * The software bill of materials section of Settings: the CycloneDX document the server serves, as a
 * table of name, version, license and kind, with a search box over name and license, the counts per
 * kind and a download link to the document itself.
 */
const { t } = useI18n();
const sbom = useSbom();
const query = ref("");
// Filtering ~300 rows is cheap, but a debounce keeps typing smooth on a slow machine
const debouncedQuery = refDebounced(query, 150);

onMounted(() => void sbom.load());

const sorted = computed(() => sortSbom(sbom.sbom.value?.components ?? []));
const rows = computed(() => filterSbom(sorted.value, debouncedQuery.value));
const counts = computed(() => countByKind(sorted.value));
const fileName = computed(() => `muxmultitool-${sbom.sbom.value?.application?.version ?? "sbom"}.cdx.json`);
</script>

<template>
	<div>
		<p class="max-w-prose text-sm text-muted">{{ t("pages.settings.sbom.description") }}</p>

		<p v-if="sbom.state.value === 'loading' || sbom.state.value === 'idle'" class="mt-3 text-sm text-muted" role="status">
			{{ t("pages.settings.sbom.loading") }}
		</p>
		<ErrorPanel v-else-if="sbom.state.value === 'error' && sbom.error.value" :error="sbom.error.value" class="mt-3" />

		<template v-else>
			<div class="mt-3 flex flex-wrap items-center gap-x-4 gap-y-2 text-sm">
				<label class="flex items-center gap-2">
					<i-ph-magnifying-glass class="size-4 text-muted" aria-hidden="true" />
					<input
						v-model="query"
						type="search"
						class="w-64 rounded-md border border-border bg-surface px-2 py-1 text-text"
						:placeholder="t('pages.settings.sbom.search')"
						:aria-label="t('pages.settings.sbom.search')"
					/>
				</label>
				<span class="text-muted">
					<template v-for="(kind, index) in SBOM_KINDS" :key="kind">
						<template v-if="index > 0"> · </template>
						{{ t(`pages.settings.sbom.count.${kind}`, counts[kind]) }}
					</template>
				</span>
				<a
					:href="SBOM_URL"
					:download="fileName"
					class="ml-auto flex items-center gap-1 text-accent underline focus-visible:outline-2 focus-visible:outline-accent"
				>
					<i-ph-download-simple class="size-4" aria-hidden="true" />
					{{ t("pages.settings.sbom.download") }}
				</a>
			</div>

			<p v-if="rows.length === 0" class="mt-3 text-sm text-muted">{{ t("pages.settings.sbom.noMatch", { query: debouncedQuery }) }}</p>
			<table v-else class="mt-3 w-full border-collapse text-sm">
				<thead class="text-left text-xs text-muted uppercase">
					<tr class="border-b border-border">
						<th scope="col" class="py-2 pr-3 font-normal">{{ t("pages.settings.sbom.columns.name") }}</th>
						<th scope="col" class="py-2 pr-3 font-normal">{{ t("pages.settings.sbom.columns.version") }}</th>
						<th scope="col" class="py-2 pr-3 font-normal">{{ t("pages.settings.sbom.columns.license") }}</th>
						<th scope="col" class="py-2 font-normal">{{ t("pages.settings.sbom.columns.kind") }}</th>
					</tr>
				</thead>
				<tbody>
					<tr v-for="row in rows" :key="`${row.kind}:${row.name}@${row.version}`" class="border-b border-border hover:bg-surface-raised">
						<td class="py-1.5 pr-3 font-mono">{{ row.name }}</td>
						<td class="py-1.5 pr-3 font-mono whitespace-nowrap">{{ row.version }}</td>
						<td class="py-1.5 pr-3">
							<template v-if="row.license">{{ row.license }}</template>
							<span v-else class="text-muted">{{ t("pages.settings.sbom.noLicense") }}</span>
						</td>
						<td class="py-1.5 whitespace-nowrap text-muted">{{ t(`pages.settings.sbom.kind.${row.kind}`) }}</td>
					</tr>
				</tbody>
			</table>
			<p v-if="sbom.sbom.value?.timestamp" class="mt-3 text-xs text-muted">
				{{ t("pages.settings.sbom.built", { time: sbom.sbom.value.timestamp }) }}
			</p>
		</template>
	</div>
</template>
