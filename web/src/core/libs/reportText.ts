import { containerLabel, formatDuration, formatGb, formatStreamSize, streamTypeName } from "~/core/libs/reportFormat";
import type { FileReport, StreamSummary } from "~/generated/muxmultitool/files/v1/files_pb";

/** How the report lists streams: under a heading per file, or as one table with the file in each row. */
export type ReportLayout = "grouped" | "flat";

/** Pads every cell of a column to the widest cell in it, so the rows line up in a monospace paste. */
export function alignColumns(rows: readonly (readonly string[])[], separator = "  "): string[] {
	const widths: number[] = [];
	for (const row of rows) {
		row.forEach((cell, index) => {
			widths[index] = Math.max(widths[index] ?? 0, cell.length);
		});
	}
	return rows.map((row) =>
		row
			.map((cell, index) => (index === row.length - 1 ? cell : cell.padEnd(widths[index] ?? 0)))
			.join(separator)
			.trimEnd()
	);
}

/** The cells of one stream row, without the file column. */
export function streamCells(stream: StreamSummary): string[] {
	return [
		`#${stream.index}`,
		streamTypeName(stream.type),
		stream.codec,
		stream.language,
		stream.title,
		formatStreamSize(stream),
		stream.flags.join(",")
	];
}

const STREAM_HEADER = ["#", "Type", "Codec", "Lang", "Title", "Size (GB)", "Flags"];

/** The name of a reported file, or its id when the file is gone. */
export function reportName(report: Pick<FileReport, "entry" | "fileId">): string {
	return report.entry?.name || report.fileId;
}

/** One line about the file as a whole: `MKV · 2:01:33 · 24.80 GB`. */
export function containerLine(report: FileReport): string {
	if (!report.container) {
		return "";
	}
	const parts = [containerLabel(report.container, reportName(report))];
	const duration = formatDuration(report.container.durationSeconds);
	if (duration) {
		parts.push(duration);
	}
	parts.push(`${formatGb(report.container.sizeBytes)} GB`);
	return parts.join(" · ");
}

/**
 * The report as plain text for pasting, the successor of the old script's output: aligned columns in
 * a monospace layout, one table per file (grouped) or one table with a file column (flat). A file
 * that failed to probe is listed with its error so the paste says what is missing.
 */
export function reportAsText(reports: readonly FileReport[], layout: ReportLayout): string {
	if (layout === "flat") {
		const rows: string[][] = [["File", ...STREAM_HEADER]];
		const failures: string[] = [];
		for (const report of reports) {
			if (report.error) {
				failures.push(`${reportName(report)}: ${report.error.message}`);
				continue;
			}
			for (const stream of report.streams) {
				rows.push([reportName(report), ...streamCells(stream)]);
			}
		}
		return [...alignColumns(rows), ...failures].join("\n");
	}

	const blocks: string[] = [];
	for (const report of reports) {
		const heading = `${reportName(report)}${report.container ? `  (${containerLine(report)})` : ""}`;
		if (report.error) {
			blocks.push(`${heading}\n  error: ${report.error.message}`);
			continue;
		}
		const rows = alignColumns([STREAM_HEADER, ...report.streams.map(streamCells)]).map((line) => `  ${line}`);
		blocks.push([heading, ...rows].join("\n"));
	}
	return blocks.join("\n\n");
}
