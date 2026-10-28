/** Version, commit and build time of the running frontend, from the vite-plugin-meta-info `<meta>` tags. */
export interface BuildInfo {
	/** From `package.json`, injected at build time. */
	version: string;
	branch?: string;
	/** Short commit hash. */
	commit?: string;
	tag?: string;
	/** The build time as the plugin printed it (`Date()` string). */
	buildTime?: string;
}

function meta(doc: Pick<Document, "querySelector">, name: string): string | undefined {
	const value = doc.querySelector(`meta[name="${name}"]`)?.getAttribute("content")?.trim();
	return value || undefined;
}

export function readBuildInfo(version: string, doc: Pick<Document, "querySelector"> = document): BuildInfo {
	return {
		version,
		branch: meta(doc, "git-branch"),
		commit: meta(doc, "git-commitHash"),
		tag: meta(doc, "git-tag"),
		buildTime: meta(doc, "build-time")
	};
}

/** The plain-text block the About screen copies for bug reports. */
export function formatBuildInfo(info: BuildInfo, ffmpegVersion?: string): string {
	const lines = [`MuxMultiTool ${info.version}`];
	if (info.commit) lines.push(`commit ${info.commit}${info.branch ? ` (${info.branch})` : ""}`);
	if (info.tag) lines.push(`tag ${info.tag}`);
	if (info.buildTime) lines.push(`built ${info.buildTime}`);
	if (ffmpegVersion) lines.push(`ffmpeg ${ffmpegVersion}`);
	return lines.join("\n");
}
