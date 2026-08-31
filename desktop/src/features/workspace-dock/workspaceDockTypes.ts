import type * as React from "react";

export type WorkspaceDockExtensionId = "business" | "life";

export type WorkspaceResource = {
  version: 1;
  extensionId?: WorkspaceDockExtensionId;
  type: string;
  id?: string;
  path: string;
  title?: string;
  metadata?: Record<string, string>;
};

export type WorkspaceDockExtension = {
  id: WorkspaceDockExtensionId;
  title: string;
  scheme: "biz" | "life";
  origin: string | null;
  homeUrl: string | null;
  resolveResource(input: string | object): WorkspaceResource | null;
  Provider: React.ComponentType<React.PropsWithChildren>;
  Dock: React.ComponentType;
  TopChromeAction: React.ComponentType;
};

export type WorkspaceDockState = {
  open: boolean;
  active: boolean;
  pinned: boolean;
  followConversation: boolean;
  fullscreen: boolean;
  currentResource: WorkspaceResource | null;
  history: WorkspaceResource[];
  dirty: boolean;
};

export type WorkspaceDockHostState = {
  activeExtensionId: WorkspaceDockExtensionId | null;
  docks: Partial<Record<WorkspaceDockExtensionId, WorkspaceDockState>>;
};
