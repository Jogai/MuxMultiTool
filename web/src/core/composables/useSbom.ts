import { ref, shallowRef } from "vue";

import { type AppError, toAppError } from "~/core/libs/errors";
import { SBOM_URL, type Sbom, parseSbom } from "~/core/libs/sbom";

/**
 * Fetches the software bill of materials the server serves at `/sbom.cdx.json` and parses it for the
 * Settings table. A plain fetch, not an RPC: the document is also what the download link points at.
 */
export function useSbom(fetchFn: typeof fetch = (...args) => fetch(...args)) {
	const state = ref<"idle" | "loading" | "ok" | "error">("idle");
	const sbom = shallowRef<Sbom>();
	const error = ref<AppError>();

	async function load() {
		state.value = "loading";
		error.value = undefined;
		try {
			const response = await fetchFn(SBOM_URL, { headers: { accept: "application/vnd.cyclonedx+json, application/json" } });
			if (!response.ok) {
				throw new Error(`${response.status} ${response.statusText}`.trim());
			}
			sbom.value = parseSbom(await response.json());
			state.value = "ok";
		} catch (cause) {
			error.value = toAppError(cause);
			state.value = "error";
		}
	}

	return { state, sbom, error, load };
}
