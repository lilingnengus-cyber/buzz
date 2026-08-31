import type {
  WorkspaceDockExtensionId,
  WorkspaceDockHostState,
  WorkspaceDockState,
} from "@/features/workspace-dock/workspaceDockTypes";

export type WorkspaceDockSwitchDecision =
  | { allowed: true; state: WorkspaceDockHostState }
  | {
      allowed: false;
      state: WorkspaceDockHostState;
      reason: "dirty-active-dock" | "unknown-extension";
    };

export function createWorkspaceDockState(): WorkspaceDockState {
  return {
    open: false,
    active: false,
    pinned: false,
    followConversation: true,
    fullscreen: false,
    currentResource: null,
    history: [],
    dirty: false,
  };
}

export function createWorkspaceDockHostState(
  extensionIds: WorkspaceDockExtensionId[],
): WorkspaceDockHostState {
  return {
    activeExtensionId: null,
    docks: Object.fromEntries(
      extensionIds.map((id) => [id, createWorkspaceDockState()]),
    ),
  };
}

export function reportWorkspaceDockState(
  state: WorkspaceDockHostState,
  extensionId: WorkspaceDockExtensionId,
  patch: Partial<Omit<WorkspaceDockState, "active">>,
): WorkspaceDockHostState {
  const current = state.docks[extensionId];
  if (!current) return state;
  return {
    ...state,
    docks: {
      ...state.docks,
      [extensionId]: { ...current, ...patch },
    },
  };
}

export function requestWorkspaceDockActivation(
  state: WorkspaceDockHostState,
  extensionId: WorkspaceDockExtensionId,
): WorkspaceDockSwitchDecision {
  if (!state.docks[extensionId]) {
    return { allowed: false, state, reason: "unknown-extension" };
  }
  const currentId = state.activeExtensionId;
  if (currentId && currentId !== extensionId && state.docks[currentId]?.dirty) {
    return { allowed: false, state, reason: "dirty-active-dock" };
  }
  return {
    allowed: true,
    state: {
      activeExtensionId: extensionId,
      docks: Object.fromEntries(
        Object.entries(state.docks).map(([id, dock]) => [
          id,
          dock ? { ...dock, active: id === extensionId } : dock,
        ]),
      ),
    },
  };
}
