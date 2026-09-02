import * as React from "react";

import { createWorkspaceDockRegistry } from "@/features/workspace-dock/WorkspaceDockRegistry";
import {
  createWorkspaceDockHostState,
  reportWorkspaceDockState,
  requestWorkspaceDockActivation,
  type WorkspaceDockSwitchDecision,
} from "@/features/workspace-dock/workspaceDockStore";
import type {
  WorkspaceDockExtension,
  WorkspaceDockExtensionId,
  WorkspaceDockHostState,
  WorkspaceDockState,
} from "@/features/workspace-dock/workspaceDockTypes";

type WorkspaceDockHostContextValue = {
  extensions: readonly WorkspaceDockExtension[];
  state: WorkspaceDockHostState;
  isActive(extensionId: WorkspaceDockExtensionId): boolean;
  reportDockState(
    extensionId: WorkspaceDockExtensionId,
    patch: Partial<Omit<WorkspaceDockState, "active">>,
  ): void;
  requestActivation(
    extensionId: WorkspaceDockExtensionId,
  ): WorkspaceDockSwitchDecision;
};

const WorkspaceDockHostContext =
  React.createContext<WorkspaceDockHostContextValue | null>(null);
const WorkspaceDockSlotContext = React.createContext<{
  active: boolean;
  extensionId: WorkspaceDockExtensionId;
} | null>(null);

export function WorkspaceDockHostProvider({
  children,
  extensions,
}: React.PropsWithChildren<{ extensions: WorkspaceDockExtension[] }>) {
  const registry = React.useMemo(
    () => createWorkspaceDockRegistry(extensions),
    [extensions],
  );
  const [state, setState] = React.useState(() =>
    createWorkspaceDockHostState(
      registry.extensions.map((extension) => extension.id),
    ),
  );
  const stateRef = React.useRef(state);
  stateRef.current = state;

  const reportDockState = React.useCallback(
    (
      extensionId: WorkspaceDockExtensionId,
      patch: Partial<Omit<WorkspaceDockState, "active">>,
    ) => {
      const next = reportWorkspaceDockState(
        stateRef.current,
        extensionId,
        patch,
      );
      if (next === stateRef.current) return;
      stateRef.current = next;
      setState(next);
    },
    [],
  );
  const requestActivation = React.useCallback(
    (extensionId: WorkspaceDockExtensionId) => {
      const decision = requestWorkspaceDockActivation(
        stateRef.current,
        extensionId,
      );
      if (decision.state !== stateRef.current) {
        stateRef.current = decision.state;
        setState(decision.state);
      }
      return decision;
    },
    [],
  );
  const value = React.useMemo<WorkspaceDockHostContextValue>(
    () => ({
      extensions: registry.extensions,
      state,
      isActive: (extensionId) => state.activeExtensionId === extensionId,
      reportDockState,
      requestActivation,
    }),
    [registry.extensions, reportDockState, requestActivation, state],
  );
  const wrapped = registry.extensions.reduceRight<React.ReactNode>(
    (content, extension) => {
      const Provider = extension.Provider;
      return <Provider key={extension.id}>{content}</Provider>;
    },
    children,
  );

  return (
    <WorkspaceDockHostContext.Provider value={value}>
      {wrapped}
    </WorkspaceDockHostContext.Provider>
  );
}

export function WorkspaceDockHost() {
  const host = useWorkspaceDockHost();
  return host.extensions.map((extension) => {
    const Dock = extension.Dock;
    return (
      <WorkspaceDockSlotContext.Provider
        key={extension.id}
        value={{
          active: host.state.activeExtensionId === extension.id,
          extensionId: extension.id,
        }}
      >
        <Dock />
      </WorkspaceDockSlotContext.Provider>
    );
  });
}

export function WorkspaceDockTopChromeActions() {
  const host = useWorkspaceDockHost();
  return host.extensions.map((extension) => {
    const Action = extension.TopChromeAction;
    return <Action key={extension.id} />;
  });
}

export function useWorkspaceDockHost() {
  const context = React.useContext(WorkspaceDockHostContext);
  if (!context)
    throw new Error(
      "useWorkspaceDockHost must be used within WorkspaceDockHostProvider",
    );
  return context;
}

export function useOptionalWorkspaceDockHost() {
  return React.useContext(WorkspaceDockHostContext);
}

export function useWorkspaceDockSlot(extensionId: WorkspaceDockExtensionId) {
  const slot = React.useContext(WorkspaceDockSlotContext);
  if (!slot || slot.extensionId !== extensionId)
    throw new Error(`Workspace dock ${extensionId} is outside its host slot`);
  return slot;
}

export function useOptionalWorkspaceDockSlot(
  extensionId: WorkspaceDockExtensionId,
) {
  const slot = React.useContext(WorkspaceDockSlotContext);
  return slot?.extensionId === extensionId ? slot : null;
}
