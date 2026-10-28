import { create } from "@bufbuild/protobuf";
import { Code, ConnectError } from "@connectrpc/connect";
import { describe, expect, it, vi } from "vitest";

import { useStreamReport } from "./useStreamReport";
import { GetStreamDetailsResponseSchema, GetStreamReportResponseSchema, StreamType } from "~/generated/muxmultitool/files/v1/files_pb";
import { fakeFilesClient } from "~/test/fakeFilesClient";

const reportOf = (ids: string[]) =>
	create(GetStreamReportResponseSchema, {
		files: ids.map((id) =>
			id === "bad"
				? { fileId: id, error: { message: "the input is not a media file ffmpeg can read" } }
				: { fileId: id, streams: [{ index: 0, type: StreamType.VIDEO, codec: "h264" }] }
		)
	});

describe("useStreamReport", () => {
	it("loads only the files not cached yet and keeps failed files next to the others", async () => {
		const client = fakeFilesClient({
			getStreamReport: vi.fn().mockImplementation(({ fileIds }: { fileIds: string[] }) => Promise.resolve(reportOf(fileIds)))
		});
		const report = useStreamReport(client);

		await report.load(["a", "bad"]);
		expect(client.getStreamReport).toHaveBeenLastCalledWith({ fileIds: ["a", "bad"], quick: true });
		expect(report.byId("a")?.report.streams).toHaveLength(1);
		expect(report.byId("bad")?.report.error?.message).toContain("not a media file");

		await report.load(["a", "bad", "c"]);
		expect(client.getStreamReport).toHaveBeenLastCalledWith({ fileIds: ["c"], quick: true });
		expect(report.reportsFor(["c", "a", "missing"]).map((file) => file.fileId)).toEqual(["c", "a"]);

		await report.load(["a", "c"]);
		expect(client.getStreamReport).toHaveBeenCalledTimes(2);
	});

	it("measures the files whose sizes are not exact yet", async () => {
		const client = fakeFilesClient({
			getStreamReport: vi.fn().mockImplementation(({ fileIds }: { fileIds: string[] }) => Promise.resolve(reportOf(fileIds)))
		});
		const report = useStreamReport(client);
		await report.load(["a", "b"]);
		expect(report.measuredAll.value).toBe(false);

		await report.measure(["a", "b"]);
		expect(client.getStreamReport).toHaveBeenLastCalledWith({ fileIds: ["a", "b"], quick: false });
		expect(report.measuredAll.value).toBe(true);

		await report.measure(["a", "b"]);
		expect(client.getStreamReport).toHaveBeenCalledTimes(2);

		report.forget(["a"]);
		expect(report.byId("a")).toBeUndefined();
		expect(report.byId("b")?.measured).toBe(true);
	});

	it("reports an error of the whole call", async () => {
		const client = fakeFilesClient({ getStreamReport: vi.fn().mockRejectedValue(new ConnectError("bad pattern", Code.InvalidArgument)) });
		const report = useStreamReport(client);
		await report.load(["a"]);
		expect(report.error.value?.codeName).toBe("invalid_argument");
		expect(report.loading.value).toBe(false);
	});

	it("fetches the details of a stream once", async () => {
		const client = fakeFilesClient({
			getStreamDetails: vi
				.fn()
				.mockResolvedValue(create(GetStreamDetailsResponseSchema, { fields: [{ key: "codec_name", value: "h264" }], json: "{}" }))
		});
		const report = useStreamReport(client);
		const first = await report.details("a", 0);
		const second = await report.details("a", 0);
		expect(first.fields[0]?.value).toBe("h264");
		expect(second).toBe(first);
		expect(client.getStreamDetails).toHaveBeenCalledTimes(1);
	});

	it("maps a details failure and retries it next time", async () => {
		const getStreamDetails = vi
			.fn()
			.mockRejectedValueOnce(new ConnectError("gone", Code.NotFound))
			.mockResolvedValue(create(GetStreamDetailsResponseSchema));
		const report = useStreamReport(fakeFilesClient({ getStreamDetails }));
		await expect(report.details("a", 1)).rejects.toMatchObject({ codeName: "not_found" });
		await expect(report.details("a", 1)).resolves.toBeDefined();
		expect(getStreamDetails).toHaveBeenCalledTimes(2);
	});
});
