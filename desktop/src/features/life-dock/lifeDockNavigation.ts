import type { WorkspaceResource } from "../workspace-dock/workspaceDockTypes";

export type LifeNavigationState = {
  entries: WorkspaceResource[];
  index: number;
};

export function createLifeNavigationState(
  initial?: WorkspaceResource | null,
): LifeNavigationState {
  return initial
    ? { entries: [initial], index: 0 }
    : { entries: [], index: -1 };
}

export function currentLifeNavigationEntry(
  state: LifeNavigationState,
): WorkspaceResource | null {
  return state.entries[state.index] ?? null;
}

export function pushLifeNavigation(
  state: LifeNavigationState,
  resource: WorkspaceResource,
): LifeNavigationState {
  if (currentLifeNavigationEntry(state)?.path === resource.path) {
    return updateCurrentLifeNavigation(state, resource);
  }
  const entries = [...state.entries.slice(0, state.index + 1), resource];
  return { entries, index: entries.length - 1 };
}

export function updateCurrentLifeNavigation(
  state: LifeNavigationState,
  resource: WorkspaceResource,
): LifeNavigationState {
  if (state.index < 0) return createLifeNavigationState(resource);
  const entries = [...state.entries];
  entries[state.index] = resource;
  return { ...state, entries };
}

export function moveLifeNavigation(
  state: LifeNavigationState,
  direction: -1 | 1,
): LifeNavigationState {
  const index = state.index + direction;
  return index >= 0 && index < state.entries.length
    ? { ...state, index }
    : state;
}

export function canMoveLifeNavigation(
  state: LifeNavigationState,
  direction: -1 | 1,
): boolean {
  const index = state.index + direction;
  return index >= 0 && index < state.entries.length;
}

export function canNavigateLifeResource(options: {
  activeExtensionId: "business" | "life" | null;
  dirty: boolean;
  followConversation: boolean;
  pinned: boolean;
  source: "explicit" | "automatic";
}): boolean {
  if (options.source === "explicit") return true;
  return (
    options.followConversation &&
    !options.pinned &&
    !options.dirty &&
    (options.activeExtensionId === null || options.activeExtensionId === "life")
  );
}
