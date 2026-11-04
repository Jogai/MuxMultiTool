import { Code, type Client } from "@connectrpc/connect";
import { computed, type MaybeRefOrGetter, onScopeDispose, ref, shallowRef, toValue, watch } from "vue";

import { type AppError, toAppError } from "~/core/libs/errors";
import { useRpcClient } from "~/core/libs/rpc";
import { CutService, type Project, SourceStatus } from "~/generated/muxmultitool/cut/v1/cut_pb";

import { type EditorState, emptyState, fromProjectState, keepIds, statesEqual, toProjectState } from "../lib/segments";

/** Where the project stands against the server. */
export type ProjectStatus = "idle" | "loading" | "saved" | "saving" | "error";

/**
 * The project of the open file: its segments and modes, saved on the server after every change with
 * undo and redo (`CutService` project methods). A change is applied here at once and saved behind the
 * scenes, in order; undo and redo take their turn in that same order and take the server's answer, so a
 * commit made while an undo is on its way is saved after it. Opening a file loads what was saved for
 * it, so the segments come back on reopen.
 */
export function useCutProject(fileId: MaybeRefOrGetter<string | undefined>, client: Client<typeof CutService> = useRpcClient(CutService)) {
	const state = ref<EditorState>(emptyState());
	const project = shallowRef<Project>();
	const status = ref<ProjectStatus>("idle");
	const error = ref<AppError>();
	let generation = 0;
	let queue: Promise<void> = Promise.resolve();
	/** Commits not yet saved, oldest first: each one is its own undo step on the server. */
	const pending: EditorState[] = [];

	const canUndo = computed(() => (project.value?.undoDepth ?? 0) > 0);
	const canRedo = computed(() => (project.value?.redoDepth ?? 0) > 0);
	const sourcePath = computed(() => project.value?.sourcePath ?? "");
	const sourceStatus = computed(() => project.value?.sourceStatus ?? SourceStatus.UNSPECIFIED);
	/** Whether the saved segments were made for another version of the file, or the file is gone. */
	const sourceChanged = computed(() => sourceStatus.value === SourceStatus.CHANGED || sourceStatus.value === SourceStatus.MISSING);

	function apply(answer: Project | undefined): void {
		if (!answer) {
			return;
		}
		project.value = answer;
		// While more commits wait, the local state is newer than the server's answer
		if (pending.length === 0) {
			state.value = keepIds(fromProjectState(answer.state), state.value);
		}
	}

	async function load(): Promise<void> {
		generation += 1;
		const mine = generation;
		pending.length = 0;
		state.value = emptyState();
		project.value = undefined;
		error.value = undefined;
		const id = toValue(fileId);
		if (!id) {
			status.value = "idle";
			return;
		}
		status.value = "loading";
		try {
			const response = await client.getProject({ fileId: id });
			if (mine !== generation) {
				return;
			}
			apply(response.project);
			status.value = "saved";
		} catch (cause) {
			if (mine === generation) {
				error.value = toAppError(cause);
				status.value = "error";
			}
		}
	}

	async function flush(): Promise<void> {
		const id = toValue(fileId);
		const mine = generation;
		const next = pending.shift();
		if (!next || !id) {
			return;
		}
		try {
			const response = await client.updateProject({ fileId: id, state: toProjectState(next) });
			if (mine !== generation) {
				return;
			}
			apply(response.project);
			error.value = undefined;
			if (pending.length === 0) {
				status.value = "saved";
			}
		} catch (cause) {
			if (mine === generation) {
				error.value = toAppError(cause);
				status.value = "error";
			}
		}
	}

	/** Applies a new state right away and saves it; a state equal to the current one is not a change. */
	function commit(next: EditorState): void {
		if (statesEqual(next, state.value)) {
			state.value = next;
			return;
		}
		state.value = next;
		pending.push(next);
		status.value = "saving";
		queue = queue.then(flush);
	}

	function step(call: () => Promise<{ project?: Project }>): Promise<void> {
		const mine = generation;
		const run = async () => {
			if (mine !== generation) {
				return;
			}
			try {
				const response = await call();
				if (mine === generation) {
					apply(response.project);
					error.value = undefined;
					if (pending.length === 0) {
						status.value = "saved";
					}
				}
			} catch (cause) {
				const failed = toAppError(cause);
				// Nothing to undo or redo (a stale button): not worth an error panel
				if (mine === generation && failed.code !== Code.FailedPrecondition) {
					error.value = failed;
				}
			}
		};
		queue = queue.then(run);
		return queue;
	}

	function undo(): Promise<void> {
		const id = toValue(fileId);
		return id ? step(() => client.undoProject({ fileId: id })) : Promise.resolve();
	}

	function redo(): Promise<void> {
		const id = toValue(fileId);
		return id ? step(() => client.redoProject({ fileId: id })) : Promise.resolve();
	}

	watch(
		() => toValue(fileId),
		() => void load(),
		{ immediate: true }
	);
	onScopeDispose(() => {
		generation += 1;
	});

	return { state, project, status, error, canUndo, canRedo, sourcePath, sourceStatus, sourceChanged, commit, undo, redo, reload: load };
}
