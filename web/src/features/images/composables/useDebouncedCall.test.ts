import { Code, ConnectError } from "@connectrpc/connect";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { nextTick, ref } from "vue";

import { useDebouncedCall } from "./useDebouncedCall";

describe("useDebouncedCall", () => {
	beforeEach(() => vi.useFakeTimers());
	afterEach(() => vi.useRealTimers());

	it("calls after a pause with the latest request and clears on none", async () => {
		const call = vi.fn().mockImplementation((value: number) => Promise.resolve(value * 2));
		const request = ref<number | undefined>(1);
		const { result, pending } = useDebouncedCall(request, call, 10);
		expect(pending.value).toBe(true);
		request.value = 2;
		await nextTick();
		await vi.advanceTimersByTimeAsync(50);
		expect(call).toHaveBeenCalledTimes(1);
		expect(call.mock.calls[0]?.[0]).toBe(2);
		expect(result.value).toBe(4);
		expect(pending.value).toBe(false);

		request.value = undefined;
		await nextTick();
		expect(result.value).toBeUndefined();
		expect(pending.value).toBe(false);
	});

	it("lets a newer call win and reports a refusal", async () => {
		let resolveFirst: ((value: string) => void) | undefined;
		const call = vi
			.fn()
			.mockImplementationOnce(
				() =>
					new Promise<string>((resolve) => {
						resolveFirst = resolve;
					})
			)
			.mockRejectedValueOnce(new ConnectError("no", Code.InvalidArgument));
		const request = ref<string | undefined>("a");
		const { result, error, pending } = useDebouncedCall(request, call, 10);
		await vi.advanceTimersByTimeAsync(20);
		request.value = "b";
		await nextTick();
		await vi.advanceTimersByTimeAsync(20);
		resolveFirst?.("stale");
		await vi.advanceTimersByTimeAsync(1);
		expect(result.value).toBeUndefined();
		expect(error.value?.message).toBe("no");
		expect(pending.value).toBe(false);
	});
});
