import type { JsonValue, Registry } from "@bufbuild/protobuf";
import { Code, ConnectError } from "@connectrpc/connect";

/**
 * An error ready for display, following the RPC convention in the architecture doc: the server puts a
 * plain-language message in the Connect error and the raw ffmpeg output in an `ErrorDetail` message.
 */
export interface AppError {
	/** Connect code; `Code.Unknown` for anything that was not a Connect error. */
	code: Code;
	/** The code's wire name, e.g. `unavailable` or `deadline_exceeded`; the i18n key under `errors.codes`. */
	codeName: string;
	/** The plain-language message, without the `[code]` prefix Connect puts in `Error.message`. */
	message: string;
	/** The raw ffmpeg output from the error detail, when the server attached one. */
	raw?: string;
	/** The original value, for logging. */
	cause: unknown;
}

/**
 * Fields of an error detail that carry the raw ffmpeg output, in order of preference.
 * The `ErrorDetail` message arrives with the jobs service; until its schema is generated the mapping
 * reads these fields from whatever detail the server attached.
 */
export const RAW_OUTPUT_FIELDS = ["raw", "rawOutput", "stderr", "output", "log"] as const;

export interface ToAppErrorOptions {
	/** Schemas to decode binary error details with; without it only JSON (development) details are read. */
	registry?: Registry;
}

/** The wire name of a Connect code: `Code.DeadlineExceeded` → `deadline_exceeded`. */
export function codeName(code: Code): string {
	return Code[code].replaceAll(/(?<=[a-z])(?=[A-Z])/g, "_").toLowerCase();
}

/**
 * Maps anything thrown by an RPC call (or by other code) to an `AppError`.
 * Non-Connect errors get `Code.Unknown` and their own message.
 */
export function toAppError(cause: unknown, { registry }: ToAppErrorOptions = {}): AppError {
	const error = ConnectError.from(cause);
	return {
		code: error.code,
		codeName: codeName(error.code),
		message: error.rawMessage,
		raw: findRawOutput(error, registry),
		cause
	};
}

/** True when a value is already an `AppError`, so callers can pass errors through unchanged. */
export function isAppError(value: unknown): value is AppError {
	return typeof value === "object" && value !== null && "codeName" in value && "message" in value && "cause" in value;
}

function findRawOutput(error: ConnectError, registry: Registry | undefined): string | undefined {
	for (const detail of error.details) {
		const raw = "type" in detail ? rawFromJson(detail.debug) : rawFromJson(detail.value as JsonValue);
		if (raw) {
			return raw;
		}
	}
	if (registry) {
		for (const message of error.findDetails(registry)) {
			const raw = rawFromJson(message as unknown as JsonValue);
			if (raw) {
				return raw;
			}
		}
	}
	return undefined;
}

function rawFromJson(value: JsonValue | undefined): string | undefined {
	if (typeof value !== "object" || value === null || Array.isArray(value)) {
		return undefined;
	}
	for (const field of RAW_OUTPUT_FIELDS) {
		const candidate = value[field];
		if (typeof candidate === "string" && candidate.length > 0) {
			return candidate;
		}
	}
	return undefined;
}
