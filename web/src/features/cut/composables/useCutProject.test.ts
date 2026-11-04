import { create, type MessageInitShape } from "@bufbuild/protobuf";
import { Code, ConnectError } from "@connectrpc/connect";
import { describe, expect, it, vi } from "vitest";
import { nextTick, ref } from "vue";

import { CutMode, ProjectSchema, type ProjectState, ProjectStateSchema, SourceStatus } from "~/generated/muxmultitool/cut/v1/cut_pb";

import { useCutProject } from "./useCutProject";
import { emptyState } from "../lib/segments";

type Client = Parameters<typeof useCutProject>[1];

/** A fake cut service with a real undo stack, enough for the composable's round trips. */
function fakeClient(initial?: MessageInitShape<typeof ProjectStateSchema>) {
	let current = create(ProjectStateSchema, initial ?? {});
	const undoStack: ProjectState[] = [];
	const redoStack: ProjectState[] = [];
	const answer = () =>
		Promise.resolve({
			project: create(ProjectSchema, {
				state: current,
				saved: true,
				undoDepth: undoStack.length,
				redoDepth: redoStack.length,
				sourceStatus: SourceStatus.UNCHANGED,
				sourcePath: "/media/a.mkv"
			})
		});
	const client = {
		getProject: vi.fn(answer),
		updateProject: vi.fn((request: { state?: MessageInitShape<typeof ProjectStateSchema> }) => {
			undoStack.push(current);
			redoStack.length = 0;
			current = create(ProjectStateSchema, request.state);
			return answer();
		}),
		undoProject: vi.fn(() => {
			const previous = undoStack.pop();
			if (!previous) {
				return Promise.reject(new ConnectError("nothing to undo", Code.FailedPrecondition));
			}
			redoStack.push(current);
			current = previous;
			return answer();
		}),
		redoProject: vi.fn(() => {
			const next = redoStack.pop();
			if (!next) {
				return Promise.reject(new ConnectError("nothing to redo", Code.FailedPrecondition));
			}
			undoStack.push(current);
			current = next;
			return answer();
		})
	};
	return client as unknown as Client & typeof client;
}

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

describe("useCutProject", () => {
	it("loads the saved project of a file with ids on its segments", async () => {
		const client = fakeClient({ segments: [{ startSeconds: 5, endSeconds: 9 }], cutMode: CutMode.NORMAL });
		const project = useCutProject(ref("/media/a.mkv"), client);
		expect(project.status.value).toBe("loading");
		await settle();
		expect(project.status.value).toBe("saved");
		expect(project.state.value.segments).toEqual([{ id: 1, start: 5, end: 9 }]);
		expect(project.state.value.cutMode).toBe(CutMode.NORMAL);
		expect(project.sourcePath.value).toBe("/media/a.mkv");
		expect(project.canUndo.value).toBe(false);
	});

	it("applies a commit at once, saves it, and undoes and redoes through the server", async () => {
		const client = fakeClient();
		const project = useCutProject(ref("/media/a.mkv"), client);
		await settle();

		project.commit({ ...emptyState(), segments: [{ id: 1, start: 1, end: 2 }] });
		expect(project.state.value.segments).toHaveLength(1);
		expect(project.status.value).toBe("saving");
		await settle();
		expect(project.status.value).toBe("saved");
		expect(client.updateProject).toHaveBeenCalledTimes(1);
		expect(project.canUndo.value).toBe(true);

		await project.undo();
		expect(project.state.value.segments).toEqual([]);
		expect(project.canUndo.value).toBe(false);
		expect(project.canRedo.value).toBe(true);

		await project.redo();
		expect(project.state.value.segments).toEqual([{ id: 1, start: 1, end: 2 }]);
	});

	it("saves commits in order and keeps the newest state while saving", async () => {
		const client = fakeClient();
		const project = useCutProject(ref("/media/a.mkv"), client);
		await settle();
		project.commit({ ...emptyState(), segments: [{ id: 1, start: 1, end: 2 }] });
		project.commit({ ...emptyState(), segments: [{ id: 1, start: 1, end: 3 }] });
		expect(project.state.value.segments[0]?.end).toBe(3);
		await settle();
		await settle();
		expect(client.updateProject).toHaveBeenCalledTimes(2);
		expect(client.updateProject.mock.calls.map((call) => call[0].state?.segments?.[0]?.endSeconds)).toEqual([2, 3]);
		expect(project.state.value.segments[0]?.end).toBe(3);
		expect(project.status.value).toBe("saved");
	});

	it("runs an undo in turn with the commits around it", async () => {
		const client = fakeClient();
		const project = useCutProject(ref("/media/a.mkv"), client);
		await settle();
		let answerUndo = () => {};
		const realUndo = client.undoProject.getMockImplementation()!;
		client.undoProject.mockImplementationOnce(() =>
			new Promise<void>((resolve) => {
				answerUndo = resolve;
			}).then(realUndo)
		);
		project.commit({ ...emptyState(), segments: [{ id: 1, start: 1, end: 2 }] });
		const undone = project.undo();
		project.commit({ ...emptyState(), segments: [{ id: 1, start: 1, end: 3 }] });
		await settle();
		// The commit made while the undo is on its way waits for the undo's answer
		expect(client.undoProject).toHaveBeenCalledTimes(1);
		expect(client.updateProject).toHaveBeenCalledTimes(1);
		expect(project.status.value).toBe("saving");
		answerUndo();
		await undone;
		await settle();
		expect(client.updateProject).toHaveBeenCalledTimes(2);
		expect(client.updateProject.mock.calls.map((call) => call[0].state?.segments?.[0]?.endSeconds)).toEqual([2, 3]);
		expect(project.state.value.segments).toEqual([{ id: 1, start: 1, end: 3 }]);
		expect(project.status.value).toBe("saved");
	});

	it("does not save a state equal to the current one", async () => {
		const client = fakeClient({ segments: [{ startSeconds: 5, endSeconds: 9 }] });
		const project = useCutProject(ref("/media/a.mkv"), client);
		await settle();
		project.commit({ ...project.state.value, segments: [{ id: 7, start: 5, end: 9 }] });
		await settle();
		expect(client.updateProject).not.toHaveBeenCalled();
	});

	it("ignores a stale undo and reports other failures", async () => {
		const client = fakeClient();
		const project = useCutProject(ref("/media/a.mkv"), client);
		await settle();
		await project.undo();
		expect(project.error.value).toBeUndefined();
		client.updateProject.mockRejectedValueOnce(new ConnectError("disk full", Code.Internal));
		project.commit({ ...emptyState(), segments: [{ id: 1, start: 1, end: 2 }] });
		await settle();
		expect(project.status.value).toBe("error");
		expect(project.error.value?.message).toBe("disk full");
	});

	it("starts over when the file changes", async () => {
		const client = fakeClient({ segments: [{ startSeconds: 5, endSeconds: 9 }] });
		const fileId = ref<string | undefined>("/media/a.mkv");
		const project = useCutProject(fileId, client);
		await settle();
		expect(project.state.value.segments).toHaveLength(1);
		fileId.value = undefined;
		await nextTick();
		expect(project.state.value.segments).toEqual([]);
		expect(project.status.value).toBe("idle");
	});
});
