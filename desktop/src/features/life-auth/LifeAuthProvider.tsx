import * as React from "react";
import type { UserManager } from "oidc-client-ts";

import {
  createWorkbenchCallbackReplayGuard,
  createWorkbenchUserManager,
  getValidWorkbenchUser,
  processWorkbenchAuthCallback,
  subscribeToDesktopAuthCallbacks,
} from "@/features/workbench-auth/workbenchAuthClient";
import type { WorkbenchAuthConfig } from "@/features/workbench-auth/workbenchAuthConfig";

import { getLifeAuthConfig } from "./lifeAuthConfig";

type LifeAuthPhase =
  | "unconnected"
  | "checking"
  | "signing-in"
  | "authenticated"
  | "expired"
  | "failed";

type LifeAuthIdentity = { subject: string; displayName: string };

type LifeAuthContextValue = {
  config: WorkbenchAuthConfig | null;
  error: string | null;
  identity: LifeAuthIdentity | null;
  phase: LifeAuthPhase;
  getAccessToken: () => Promise<string | null>;
  signIn: () => Promise<void>;
  signOut: () => Promise<void>;
};

const LifeAuthContext = React.createContext<LifeAuthContextValue | null>(null);

function identityFromProfile(
  profile: Record<string, unknown>,
): LifeAuthIdentity {
  const subject = typeof profile.sub === "string" ? profile.sub : "";
  const displayName = [profile.name, profile.preferred_username, subject].find(
    (value): value is string => typeof value === "string" && value.length > 0,
  );
  return { subject, displayName: displayName ?? "Authenticated user" };
}

export function LifeAuthProvider({ children }: React.PropsWithChildren) {
  const result = React.useMemo(getLifeAuthConfig, []);
  const manager = React.useMemo<UserManager | null>(
    () =>
      result.config && typeof window !== "undefined"
        ? createWorkbenchUserManager(
            result.config,
            window.sessionStorage,
            "life-workbench",
          )
        : null,
    [result.config],
  );
  const [phase, setPhase] = React.useState<LifeAuthPhase>(
    result.config ? "checking" : result.error ? "failed" : "unconnected",
  );
  const [error, setError] = React.useState<string | null>(result.error);
  const [identity, setIdentity] = React.useState<LifeAuthIdentity | null>(null);
  const callbackReplayGuard = React.useRef(
    createWorkbenchCallbackReplayGuard(),
  );

  const applyUser = React.useCallback(
    (user: Awaited<ReturnType<UserManager["getUser"]>>) => {
      const active = user && !user.expired ? user : null;
      setIdentity(active ? identityFromProfile(active.profile) : null);
      setPhase(active ? "authenticated" : user ? "expired" : "unconnected");
    },
    [],
  );

  const consumeCallback = React.useCallback(
    async (url: string) => {
      if (!manager || !result.config) return;
      if (!callbackReplayGuard.current.accept(url)) return;
      try {
        const processed = await processWorkbenchAuthCallback(
          manager,
          result.config,
          url,
        );
        if (!processed) return;
        applyUser(await manager.getUser());
        setError(null);
      } catch (cause) {
        setPhase("failed");
        setError(
          cause instanceof Error ? cause.message : "Life OIDC callback failed.",
        );
      }
    },
    [applyUser, manager, result.config],
  );

  React.useEffect(() => {
    if (!manager || !result.config) return;
    let disposed = false;
    let cleanup: () => void = () => undefined;
    void (async () => {
      const stored = await manager.getUser();
      const user = stored ? await getValidWorkbenchUser(manager) : null;
      if (disposed) return;
      applyUser(user ?? stored);
      const unsubscribe = await subscribeToDesktopAuthCallbacks((url) => {
        void consumeCallback(url);
      });
      if (disposed) unsubscribe();
      else cleanup = unsubscribe;
    })().catch((cause) => {
      if (disposed) return;
      setPhase("failed");
      setError(
        cause instanceof Error
          ? cause.message
          : "Life OIDC initialization failed.",
      );
    });
    return () => {
      disposed = true;
      cleanup();
    };
  }, [applyUser, consumeCallback, manager, result.config]);

  React.useEffect(() => {
    if (!manager) return;
    const userLoaded = (user: Awaited<ReturnType<UserManager["getUser"]>>) => {
      applyUser(user);
      setError(null);
    };
    const silentRenewError = () => {
      void manager.getUser().then(applyUser);
    };
    manager.events.addUserLoaded(userLoaded);
    manager.events.addSilentRenewError(silentRenewError);
    return () => {
      manager.events.removeUserLoaded(userLoaded);
      manager.events.removeSilentRenewError(silentRenewError);
    };
  }, [applyUser, manager]);

  const getAccessToken = React.useCallback(async () => {
    if (import.meta.env.MODE === "e2e") {
      const injected = window.__BUZZ_E2E_LIFE_ACCESS_TOKEN__;
      if (typeof injected === "string" && injected.length > 0) return injected;
    }
    if (!manager) return null;
    const user = await getValidWorkbenchUser(manager);
    applyUser(user);
    return user?.access_token ?? null;
  }, [applyUser, manager]);

  const value = React.useMemo<LifeAuthContextValue>(
    () => ({
      config: result.config,
      error,
      identity,
      phase,
      getAccessToken,
      signIn: async () => {
        if (!manager) return;
        setPhase("signing-in");
        setError(null);
        try {
          await manager.signinRedirect();
        } catch (cause) {
          setPhase("failed");
          setError(
            cause instanceof Error ? cause.message : "Life sign-in failed.",
          );
        }
      },
      signOut: async () => {
        if (!manager) return;
        setError(null);
        try {
          await manager.signoutRedirect();
        } catch (cause) {
          setPhase("failed");
          setError(
            cause instanceof Error ? cause.message : "Life sign-out failed.",
          );
        }
      },
    }),
    [error, getAccessToken, identity, manager, phase, result.config],
  );

  return (
    <LifeAuthContext.Provider value={value}>
      {children}
    </LifeAuthContext.Provider>
  );
}

export function useLifeAuth(): LifeAuthContextValue {
  const value = React.useContext(LifeAuthContext);
  if (!value)
    throw new Error("useLifeAuth must be used within LifeAuthProvider");
  return value;
}
