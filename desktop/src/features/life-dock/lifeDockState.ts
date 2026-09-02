import type { LifeDockConfig } from "./lifeDockConfig";
import type { LifeDockPreferences } from "./lifeDockPreferences";
import type { WorkspaceResource } from "../workspace-dock/workspaceDockTypes";

export type LifeDockState = {
  browserMounted: boolean;
  currentUrl: string;
  currentResource: WorkspaceResource | null;
  dirty: boolean;
  followConversation: boolean;
  fullscreen: boolean;
  frameUrl: string;
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
  | { type: "load-frame"; url: string }
  | { type: "dirty"; dirty: boolean }
  | { type: "title"; title?: string }
  | {
      type: "navigate" | "sync-resource";
      url: string;
      resource: WorkspaceResource;
    };

export function createInitialLifeDockState(
  config: LifeDockConfig | null,
  preferences: LifeDockPreferences,
): LifeDockState {
  return {
    browserMounted: false,
    currentUrl: config?.homeUrl ?? "",
    currentResource: null,
    dirty: false,
    followConversation: preferences.followConversation,
    fullscreen: false,
    frameUrl: "about:blank",
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
      return { ...state, browserMounted: true, open: true };
    case "close":
      return { ...state, open: false, fullscreen: false };
    case "toggle-fullscreen":
      return {
        ...state,
        browserMounted: true,
        open: true,
        fullscreen: !state.fullscreen,
      };
    case "exit-fullscreen":
      return { ...state, fullscreen: false };
    case "toggle-pinned":
      return { ...state, pinned: !state.pinned };
    case "toggle-follow":
      return { ...state, followConversation: !state.followConversation };
    case "loading":
      return { ...state, loading: action.loading };
    case "load-frame":
      return {
        ...state,
        browserMounted: true,
        frameUrl: action.url,
        loading: true,
      };
    case "dirty":
      return { ...state, dirty: action.dirty };
    case "title":
      return { ...state, title: action.title };
    case "navigate":
      return {
        ...state,
        browserMounted: true,
        currentResource: action.resource,
        currentUrl: action.url,
        dirty: false,
        loading: true,
        open: true,
      };
    case "sync-resource":
      return {
        ...state,
        currentResource: action.resource,
        currentUrl: action.url,
        dirty: false,
        loading: false,
      };
  }
}
