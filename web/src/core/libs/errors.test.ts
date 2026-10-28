import { create, createFileRegistry, toBinary } from "@bufbuild/protobuf";
import { FieldDescriptorProto_Type, FileDescriptorProtoSchema, StructSchema } from "@bufbuild/protobuf/wkt";
import { Code, ConnectError } from "@connectrpc/connect";
import { describe, expect, it } from "vitest";

import { codeName, isAppError, toAppError } from "./errors";

const FFMPEG_OUTPUT = "av_interleaved_write_frame(): No space left on device\nConversion failed!";

// Stand-in for the jobs service's ErrorDetail, whose schema is generated once TASK-15 lands
const registry = createFileRegistry(
	create(FileDescriptorProtoSchema, {
		name: "error.proto",
		package: "muxmultitool.jobs.v1",
		messageType: [{ name: "ErrorDetail", field: [{ name: "raw", number: 1, type: FieldDescriptorProto_Type.STRING, jsonName: "raw" }] }]
	}),
	() => undefined
);
const ErrorDetailSchema = registry.getMessage("muxmultitool.jobs.v1.ErrorDetail")!;

describe("codeName", () => {
	it("gives the wire name of a code", () => {
		expect(codeName(Code.Unavailable)).toBe("unavailable");
		expect(codeName(Code.DeadlineExceeded)).toBe("deadline_exceeded");
		expect(codeName(Code.FailedPrecondition)).toBe("failed_precondition");
	});
});

describe("toAppError", () => {
	it("keeps the plain-language message without the code prefix", () => {
		const error = toAppError(new ConnectError("The output disk is full.", Code.ResourceExhausted));

		expect(error).toMatchObject({ code: Code.ResourceExhausted, codeName: "resource_exhausted", message: "The output disk is full." });
		expect(error.raw).toBeUndefined();
	});

	it("reads the raw ffmpeg output from a JSON error detail, as sent in development", () => {
		const cause = new ConnectError("The output disk is full.", Code.ResourceExhausted);
		cause.details = [{ type: "muxmultitool.jobs.v1.ErrorDetail", value: new Uint8Array(), debug: { raw: FFMPEG_OUTPUT } }];

		expect(toAppError(cause).raw).toBe(FFMPEG_OUTPUT);
	});

	it("reads the raw ffmpeg output from an outgoing detail message", () => {
		const cause = new ConnectError("Merge failed.", Code.Internal, undefined, [
			{ desc: StructSchema, value: create(StructSchema, { fields: {} }) }
		]);
		// Outgoing details are plain objects; the mapping looks at their raw-output fields
		cause.details = [{ desc: StructSchema, value: { stderr: FFMPEG_OUTPUT } }];

		expect(toAppError(cause).raw).toBe(FFMPEG_OUTPUT);
	});

	it("decodes binary details with the ErrorDetail schema from a registry", () => {
		const binary = toBinary(ErrorDetailSchema, create(ErrorDetailSchema, { raw: FFMPEG_OUTPUT }));
		const cause = new ConnectError("Merge failed.", Code.Internal);
		cause.details = [{ type: ErrorDetailSchema.typeName, value: binary }];

		expect(toAppError(cause).raw).toBeUndefined();
		expect(toAppError(cause, { registry }).raw).toBe(FFMPEG_OUTPUT);
	});

	it("skips details without raw output", () => {
		const cause = new ConnectError("Not found.", Code.NotFound);
		cause.details = [
			{ type: "some.Other", value: new Uint8Array(), debug: { reason: "x" } },
			{ type: "a.B", value: new Uint8Array() }
		];

		expect(toAppError(cause).raw).toBeUndefined();
	});

	it("maps a plain Error to the unknown code", () => {
		const cause = new TypeError("Failed to fetch");
		const error = toAppError(cause);

		expect(error).toMatchObject({ code: Code.Unknown, codeName: "unknown", message: "Failed to fetch", cause });
	});

	it("maps a thrown non-Error value", () => {
		expect(toAppError("boom")).toMatchObject({ code: Code.Unknown, message: "boom", cause: "boom" });
	});

	it("maps a fetch abort to canceled", () => {
		const cause = new DOMException("The user aborted a request.", "AbortError");
		expect(toAppError(cause).code).toBe(Code.Canceled);
	});
});

describe("isAppError", () => {
	it("recognises mapped errors and nothing else", () => {
		expect(isAppError(toAppError(new Error("x")))).toBe(true);
		expect(isAppError(new Error("x"))).toBe(false);
		expect(isAppError(null)).toBe(false);
	});
});
