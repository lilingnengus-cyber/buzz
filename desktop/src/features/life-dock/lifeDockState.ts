import type { LifeDockConfig } from "./lifeDockConfig";
import type { LifeDockPreferences } from "./lifeDockPreferences";
import type { WorkspaceResource } from "../workspace-dock/workspaceDockTypes";

export type LifeDockState = {
  currentUrl: string;
  currentResource: WorkspaceResource | null;
  dirty: boolean;
  followConversation: boolean;
  fullscreen: boolean;
  loading: boolean;
  open: boolean;
  pinned: boolean;
  title?: string;
};

export type LifeDockAction =
  | {
      type:
        | "open"
        | "close"
        | "toggle-fullscreen"
        | "exit-fullscreen"
        | "toggle-pinned"
        | "toggle-follow";
    }
  | { type: "loading"; loading: boolean }
  | { type: "dirty"; dirty: boolean }
  | { type: "title"; title?: string }
  | { type: "navigate"; url: string; resource: WorkspaceResource };

export function createInitialLifeDockState(
  config: LifeDockConfig | null,
  preferences: LifeDockPreferences,
): LifeDockState {
  return {
    currentUrl: config?.homeUrl ?? "",
    currentResource: null,
    dirty: false,
    followConversation: preferences.followConversation,
    fullscreen: false,
    loading: false,
    open: false,
    pinned: preferences.pinned,
  };
}

export function lifeDockReducer(
  state: LifeDockState,
  action: LifeDockAction,
): LifeDockState {
  switch (action.type) {
    case "open":
      return { ...state, open: true };
    case "close":
      return { ...state, open: false, fullscreen: false };
    case "toggle-fullscreen":
      return { ...state, open: true, fullscreen: !state.fullscreen };
    case "exit-fullscreen":
      return { ...state, fullscreen: false };
    case "toggle-pinned":
      return { ...state, pinned: !state.pinned };
    case "toggle-follow":
      return { ...state, followConversation: !state.followConversation };
    case "loading":
      return { ...state, loading: action.loading };
    case "dirty":
      return { ...state, dirty: action.dirty };
    case "title":
      return { ...state, title: action.title };
    case "navigate":
      return {
        ...state,
        currentResource: action.resource,
        currentUrl: action.url,
        dirty: false,
        loading: true,
        open: true,
      };
  }
}
