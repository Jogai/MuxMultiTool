/** `movie` for `movie.mkv`; a name that is all extension (`.hidden`) or has none stays whole. */
export function stem(name: string): string {
	const dot = name.lastIndexOf(".");
	return dot > 0 ? name.slice(0, dot) : name;
}

/** `mkv` for `movie.mkv` (no dot); empty when the name has no extension. */
export function extensionOf(name: string): string {
	const dot = name.lastIndexOf(".");
	return dot > 0 ? name.slice(dot + 1) : "";
}
