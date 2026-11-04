import { type MaybeRefOrGetter, onScopeDispose, ref, shallowRef, toValue, watch } from "vue";

import { type AppError, toAppError } from "~/core/libs/errors";

/**
 * Runs an idempotent RPC whenever its request changes, after a short pause, so a panel always shows
 * what a submit would do (the GIF estimate, the crop plan). A newer call wins over one still in
 * flight, which is aborted; no request (undefined) clears the result.
 */
export function useDebouncedCall<Request, Response>(
	request: MaybeRefOrGetter<Request | undefined>,
	call: (request: Request, signal: AbortSignal) => Promise<Response>,
	delayMs = 200
) {
	const result = shallowRef<Response>();
	const pending = ref(false);
	const error = ref<AppError>();
	let latest = 0;
	let controller: AbortController | undefined;
	let timer: ReturnType<typeof setTimeout> | undefined;

	async function run(value: Request): Promise<void> {
		const mine = ++latest;
		controller?.abort();
		const abort = new AbortController();
		controller = abort;
		pending.value = true;
		try {
			const response = await call(value, abort.signal);
			if (mine === latest) {
				result.value = response;
				error.value = undefined;
			}
		} catch (cause) {
			if (mine === latest && !abort.signal.aborted) {
				result.value = undefined;
				error.value = toAppError(cause);
			}
		} finally {
			if (mine === latest) {
				pending.value = false;
			}
		}
	}

	function cancel(): void {
		clearTimeout(timer);
		latest++;
		controller?.abort();
		pending.value = false;
	}

	watch(
		() => toValue(request),
		(value) => {
			clearTimeout(timer);
			if (value === undefined) {
				cancel();
				result.value = undefined;
				error.value = undefined;
				return;
			}
			pending.value = true;
			timer = setTimeout(() => void run(value), delayMs);
		},
		{ deep: true, immediate: true }
	);
	onScopeDispose(cancel);

	/** Runs again right away for the current request, e.g. after a submit that may have changed the server's answer. */
	function refresh(): void {
		clearTimeout(timer);
		const value = toValue(request);
		if (value !== undefined) {
			void run(value);
		}
	}

	return { result, pending, error, refresh };
}
