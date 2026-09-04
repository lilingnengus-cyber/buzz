import { openUrl } from "@tauri-apps/plugin-opener";
import * as React from "react";
import { toast } from "sonner";

import {
  createLifeIdentityBindingChallenge,
  createLifeWorkbenchSession,
  getLifeWorkbenchAccount,
  issueLifeEmbedSession,
  readOidcNonce,
  revokeLifeEmbedSession,
  verifyLifeIdentityBinding,
} from "./lifeAuthGateway";
import {
  createLifeBridgeMessage,
  createLifeSessionNonce,
  readLifeBridgeEvent,
} from "./lifeDockBridge";
import {
  getLifeAuthGatewayUrl,
  getLifeDockConfig,
  type LifeDockConfig,
} from "./lifeDockConfig";
import {
  canNavigateLifeResource,
  canMoveLifeNavigation,
  createLifeNavigationState,
  currentLifeNavigationEntry,
  moveLifeNavigation,
  pushLifeNavigation,
  updateCurrentLifeNavigation,
} from "./lifeDockNavigation";
import {
  DEFAULT_LIFE_DOCK_PREFERENCES,
  readLifeDockPreferences,
  saveLifeDockPreferences,
} from "./lifeDockPreferences";
import { createInitialLifeDockState, lifeDockReducer } from "./lifeDockState";
import {
  buildLifeReference,
  buildLifeUrl,
  parseLifeUrl,
  resolveLifeResource,
} from "./lifeResourceResolver";
import { useLifeDockWidth } from "./useLifeDockWidth";
import {
  canAttemptLifeRecovery,
  lifeSessionRenewalDelay,
  readLifeEmbedCode,
  validateLifeEmbedUrl,
} from "./lifeEmbedSession";
import { useLifeAuth } from "../life-auth";
import { useOptionalWorkspaceDockHost } from "../workspace-dock";
import type { WorkspaceResource } from "../workspace-dock/workspaceDockTypes";
import { useTheme } from "../../shared/theme/ThemeProvider";
import { signRelayEvent } from "../../shared/api/tauri";
import { getIdentity } from "../../shared/api/tauriIdentity";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "../../shared/ui/alert-dialog";

export const LIFE_DATA_CHANGED_EVENT = "buzz:life-data-changed";

type LifeAuthState = {
  phase: "unconnected" | "checking" | "authenticated" | "expired" | "failed";
  displayName?: string;
  reason?: string;
};

type LifeDockContextValue = {
  active: boolean;
  auth: LifeAuthState;
  bridgeReady: boolean;
  canGoBack: boolean;
  canGoForward: boolean;
  canResetWidth: boolean;
  close(): void;
  config: LifeDockConfig | null;
  configError: string | null;
  goBack(): void;
  goForward(): void;
  goHome(): void;
  iframeRef: React.RefObject<HTMLIFrameElement | null>;
  isOverlay: boolean;
  logout(): void;
  onBrowserLoad(): void;
  onResetWidth(): void;
  onResizeStart: React.MouseEventHandler<HTMLButtonElement>;
  openCurrentInBrowser(): void;
  openLifeResource(resource: WorkspaceResource): void;
  openLifeResourceAutomatically(resource: WorkspaceResource): boolean;
  openLifeResourceInBrowser(resource: WorkspaceResource): void;
  refresh(): void;
  renderedWidthPx: number;
  resolveLifeResourceLink(value: string): WorkspaceResource | null;
  startSession(): void;
  state: ReturnType<typeof createInitialLifeDockState>;
  toggle(): void;
  toggleFollowConversation(): void;
  toggleFullscreen(): void;
  togglePinned(): void;
  lifeAuthPhase: ReturnType<typeof useLifeAuth>["phase"];
};

const LifeDockContext = React.createContext<LifeDockContextValue | null>(null);

function storage(): Storage | null {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

export function LifeDockProvider({ children }: React.PropsWithChildren) {
  const configResult = React.useMemo(getLifeDockConfig, []);
  const config = configResult.config;
  const gateway = React.useMemo(getLifeAuthGatewayUrl, []);
  const preferences = React.useMemo(
    () =>
      typeof window === "undefined"
        ? DEFAULT_LIFE_DOCK_PREFERENCES
        : readLifeDockPreferences(storage()),
    [],
  );
  const [state, dispatch] = React.useReducer(lifeDockReducer, undefined, () =>
    createInitialLifeDockState(config, preferences),
  );
  const homeResource = React.useMemo(
    () => resolveLifeResource("life://dashboard"),
    [],
  );
  const [navigation, setNavigation] = React.useState(() =>
    createLifeNavigationState(homeResource),
  );
  const [auth, setAuth] = React.useState<LifeAuthState>({
    phase: "unconnected",
  });
  const [sessionExpiresAt, setSessionExpiresAt] = React.useState<string | null>(
    null,
  );
  const [bridgeReady, setBridgeReady] = React.useState(false);
  const [leaveConfirmationOpen, setLeaveConfirmationOpen] =
    React.useState(false);
  const iframeRef = React.useRef<HTMLIFrameElement>(null);
  const nonceRef = React.useRef(createLifeSessionNonce());
  const stateRef = React.useRef(state);
  const navigationRef = React.useRef(navigation);
  const pendingNavigationRef = React.useRef<WorkspaceResource | null>(null);
  const pendingLeaveRef = React.useRef<(() => void) | null>(null);
  const workbenchSessionTokenRef = React.useRef<string | null>(null);
  const embedSessionIdRef = React.useRef<string | null>(null);
  const recoveryAttemptsRef = React.useRef(0);
  const sessionStartingRef = React.useRef(false);
  const manuallyDisconnectedRef = React.useRef(false);
  const bridgeHandshakeTimerRef = React.useRef<number | null>(null);
  const lifeAuth = useLifeAuth();
  const host = useOptionalWorkspaceDockHost();
  const reportDockState = host?.reportDockState;
  const requestDockActivation = host?.requestActivation;
  const activeExtensionId = host?.state.activeExtensionId ?? null;
  const { isDark } = useTheme();
  const width = useLifeDockWidth();
  const active = activeExtensionId === "life" || (!host && state.open);

  React.useEffect(() => {
    stateRef.current = state;
  }, [state]);
  React.useEffect(() => {
    navigationRef.current = navigation;
  }, [navigation]);
  React.useEffect(() => {
    reportDockState?.("life", {
      open: state.open,
      pinned: state.pinned,
      followConversation: state.followConversation,
      fullscreen: state.fullscreen,
      currentResource: state.currentResource,
      history: navigation.entries,
      dirty: state.dirty,
    });
  }, [
    navigation.entries,
    reportDockState,
    state.currentResource,
    state.dirty,
    state.followConversation,
    state.fullscreen,
    state.open,
    state.pinned,
  ]);
  React.useEffect(() => {
    saveLifeDockPreferences(storage(), {
      followConversation: state.followConversation,
      pinned: state.pinned,
    });
  }, [state.followConversation, state.pinned]);

  const post = React.useCallback(
    (
      type: Parameters<typeof createLifeBridgeMessage>[0],
      payload?: unknown,
    ) => {
      if (!config || !iframeRef.current?.contentWindow) return false;
      iframeRef.current.contentWindow.postMessage(
        createLifeBridgeMessage(type, nonceRef.current, payload),
        config.origin,
      );
      return true;
    },
    [config],
  );
  const stopBridgeHandshake = React.useCallback(() => {
    if (bridgeHandshakeTimerRef.current === null) return;
    window.clearInterval(bridgeHandshakeTimerRef.current);
    bridgeHandshakeTimerRef.current = null;
  }, []);
  const startBridgeHandshake = React.useCallback(() => {
    stopBridgeHandshake();
    post("HOST_INIT", { hostVersion: 2 });
    let attempts = 1;
    bridgeHandshakeTimerRef.current = window.setInterval(() => {
      attempts += 1;
      if (attempts > 40) {
        stopBridgeHandshake();
        return;
      }
      post("HOST_INIT", { hostVersion: 2 });
    }, 250);
  }, [post, stopBridgeHandshake]);

  const requestLeave = React.useCallback((action: () => void) => {
    if (!stateRef.current.dirty) {
      action();
      return;
    }
    pendingLeaveRef.current = action;
    setLeaveConfirmationOpen(true);
  }, []);

  const sendNavigation = React.useCallback(
    (resource: WorkspaceResource) => {
      if (!config) return;
      if (bridgeReady && post("NAVIGATE", { path: resource.path })) return;
      pendingNavigationRef.current = resource;
      const url = buildLifeUrl(resource, config);
      if (url) dispatch({ type: "load-frame", url });
    },
    [bridgeReady, config, post],
  );

  const commitNavigation = React.useCallback(
    (
      resource: WorkspaceResource,
      mode: "push" | "replace" | "history" = "push",
    ) => {
      if (!config) return;
      const reference = buildLifeReference(resource);
      const normalized = reference ? resolveLifeResource(reference) : null;
      const url = normalized ? buildLifeUrl(normalized, config) : null;
      if (!normalized || !url) return;
      if (mode === "push")
        setNavigation((current) => pushLifeNavigation(current, normalized));
      else if (mode === "replace")
        setNavigation((current) =>
          updateCurrentLifeNavigation(current, normalized),
        );
      dispatch({ type: "navigate", url, resource: normalized });
      sendNavigation(normalized);
    },
    [config, sendNavigation],
  );

  const openLifeResource = React.useCallback(
    (resource: WorkspaceResource) => {
      if (
        !canNavigateLifeResource({
          activeExtensionId,
          dirty: stateRef.current.dirty,
          followConversation: stateRef.current.followConversation,
          pinned: stateRef.current.pinned,
          source: "explicit",
        })
      )
        return;
      requestLeave(() => commitNavigation(resource));
    },
    [activeExtensionId, commitNavigation, requestLeave],
  );

  const startLifeSession = React.useCallback(
    (automatic = false, renewExisting = false) => {
      if (!config || !gateway || sessionStartingRef.current) return;
      sessionStartingRef.current = true;
      if (!renewExisting) setAuth({ phase: "checking" });
      void (async () => {
        const oidcToken = await lifeAuth.getAccessToken();
        if (!oidcToken) {
          if (!automatic) {
            await lifeAuth.signIn();
            return;
          }
          throw new Error("Life OIDC session expired.");
        }
        let sessionToken = workbenchSessionTokenRef.current;
        if (!sessionToken) {
          const idToken = await lifeAuth.getIdToken();
          if (!readOidcNonce(idToken ?? oidcToken)) {
            if (!automatic) {
              await lifeAuth.signIn();
              return;
            }
            throw new Error("Workbench OIDC nonce is unavailable.");
          }
          const session = await createLifeWorkbenchSession(
            gateway,
            oidcToken,
            idToken,
          );
          sessionToken = session.sessionToken;
          workbenchSessionTokenRef.current = sessionToken;
          setSessionExpiresAt(session.expiresAt);
        }
        const identity = await getIdentity();
        const account = await getLifeWorkbenchAccount(gateway, sessionToken);
        const alreadyBound = account.bindings.some(
          (binding) =>
            binding.pubkey === identity.pubkey && binding.status === "active",
        );
        if (!alreadyBound) {
          const challenge = await createLifeIdentityBindingChallenge(
            gateway,
            sessionToken,
            identity.pubkey,
          );
          const signedEvent = await signRelayEvent({
            kind: 24243,
            content: challenge.canonicalPayload,
            createdAt: challenge.createdAt,
            tags: [],
          });
          const binding = await verifyLifeIdentityBinding(
            gateway,
            sessionToken,
            challenge.challengeId,
            signedEvent,
          );
          if (binding.pubkey !== identity.pubkey || binding.status !== "active")
            throw new Error("Life identity binding was not activated.");
        }
        const target = stateRef.current.currentResource ?? homeResource;
        if (!target) throw new Error("LifeOS home resource is unavailable.");
        const issued = await issueLifeEmbedSession(
          gateway,
          sessionToken,
          target,
        );
        const embedUrl = validateLifeEmbedUrl(config, issued.embedUrl);
        if (!embedUrl) throw new Error("LifeOS bootstrap URL was rejected.");
        embedSessionIdRef.current = issued.embedSessionId;
        if (renewExisting && bridgeReady) {
          const code = readLifeEmbedCode(config, embedUrl);
          if (code && post("RENEW_SESSION", { code })) return;
        }
        dispatch({ type: "load-frame", url: embedUrl });
      })()
        .catch((cause) => {
          workbenchSessionTokenRef.current = null;
          setSessionExpiresAt(null);
          setAuth({
            phase: automatic ? "expired" : "failed",
            reason:
              cause instanceof Error ? cause.message : "LifeOS sign-in failed.",
          });
        })
        .finally(() => {
          sessionStartingRef.current = false;
        });
    },
    [bridgeReady, config, gateway, homeResource, lifeAuth, post],
  );

  React.useEffect(() => {
    if (!state.open) return;
    const decision = requestDockActivation?.("life");
    if (decision && !decision.allowed) {
      dispatch({ type: "close" });
      toast.warning("Save or discard changes in the active workspace first.");
    }
  }, [requestDockActivation, state.open]);
  React.useEffect(() => {
    if (
      state.open &&
      active &&
      auth.phase === "unconnected" &&
      !manuallyDisconnectedRef.current
    )
      startLifeSession(true);
  }, [active, auth.phase, startLifeSession, state.open]);
  React.useEffect(() => {
    if (
      lifeAuth.phase === "authenticated" ||
      lifeAuth.phase === "checking" ||
      lifeAuth.phase === "signing-in"
    )
      return;
    workbenchSessionTokenRef.current = null;
    setSessionExpiresAt(null);
    embedSessionIdRef.current = null;
    recoveryAttemptsRef.current = 0;
    if (lifeAuth.phase === "expired") {
      setAuth({ phase: "expired", reason: "Life OIDC session expired." });
      return;
    }
    manuallyDisconnectedRef.current = false;
    setAuth({ phase: "unconnected" });
    post("LOGOUT");
  }, [lifeAuth.phase, post]);
  React.useEffect(() => {
    if (
      auth.phase !== "expired" ||
      !canAttemptLifeRecovery(recoveryAttemptsRef.current)
    )
      return;
    recoveryAttemptsRef.current += 1;
    workbenchSessionTokenRef.current = null;
    startLifeSession(true);
  }, [auth.phase, startLifeSession]);
  React.useEffect(() => {
    if (
      !state.open ||
      !active ||
      !bridgeReady ||
      auth.phase !== "authenticated" ||
      !sessionExpiresAt
    )
      return;
    const delay = lifeSessionRenewalDelay(sessionExpiresAt);
    if (delay === null) return;
    const timer = window.setTimeout(() => {
      workbenchSessionTokenRef.current = null;
      setSessionExpiresAt(null);
      startLifeSession(true, true);
    }, delay);
    return () => window.clearTimeout(timer);
  }, [
    active,
    auth.phase,
    bridgeReady,
    sessionExpiresAt,
    startLifeSession,
    state.open,
  ]);
  React.useEffect(() => {
    if (!state.open || auth.phase !== "authenticated") return;
    const timer = window.setInterval(() => post("CHECK_AUTH"), 60_000);
    return () => window.clearInterval(timer);
  }, [auth.phase, post, state.open]);

  React.useEffect(() => {
    if (!config) return;
    const onMessage = (event: MessageEvent) => {
      const message = readLifeBridgeEvent(
        event,
        iframeRef.current?.contentWindow ?? null,
        config,
        nonceRef.current,
      );
      if (!message) return;
      if (message.type === "LIFE_READY") {
        stopBridgeHandshake();
        setBridgeReady(true);
        post("SET_THEME", { theme: isDark ? "dark" : "light" });
        post("CHECK_AUTH");
        const pending = pendingNavigationRef.current;
        if (pending) {
          pendingNavigationRef.current = null;
          post("NAVIGATE", { path: pending.path });
        }
      } else if (message.type === "AUTH_STATUS") {
        if (message.payload.authenticated) {
          recoveryAttemptsRef.current = 0;
          setAuth({
            phase: "authenticated",
            displayName: message.payload.user.displayName,
          });
        } else setAuth({ phase: "unconnected" });
      } else if (message.type === "AUTH_REQUIRED") {
        setAuth({ phase: "unconnected", reason: message.payload.reason });
      } else if (message.type === "SESSION_EXPIRED") {
        setAuth({ phase: "expired", reason: message.payload.reason });
      } else if (message.type === "TITLE_CHANGED") {
        dispatch({ type: "title", title: message.payload.title });
      } else if (message.type === "DIRTY_STATE_CHANGED") {
        dispatch({ type: "dirty", dirty: message.payload.dirty });
      } else if (message.type === "RESOURCE_CHANGED") {
        const url = buildLifeUrl(message.payload.resource, config);
        if (!url) return;
        dispatch({
          type: "sync-resource",
          url,
          resource: message.payload.resource,
        });
        setNavigation((current) =>
          updateCurrentLifeNavigation(current, message.payload.resource),
        );
      } else if (message.type === "ROUTE_CHANGED") {
        const resource = parseLifeUrl(message.payload.url, config);
        const url = resource ? buildLifeUrl(resource, config) : null;
        if (!resource || !url) return;
        dispatch({ type: "sync-resource", url, resource });
        setNavigation((current) =>
          updateCurrentLifeNavigation(current, resource),
        );
      } else if (
        message.type === "ACTION_COMPLETED" ||
        message.type === "ACTION_FAILED"
      ) {
        if (message.type === "ACTION_COMPLETED")
          toast.success(message.payload.message);
        else toast.error(message.payload.message);
      } else if (message.type === "DATA_CHANGED") {
        window.dispatchEvent(
          new CustomEvent(LIFE_DATA_CHANGED_EVENT, {
            detail: message.payload.resource
              ? {
                  type: message.payload.resource.type,
                  id: message.payload.resource.id,
                }
              : {},
          }),
        );
      }
    };
    window.addEventListener("message", onMessage);
    return () => window.removeEventListener("message", onMessage);
  }, [config, isDark, post, stopBridgeHandshake]);
  React.useEffect(() => () => stopBridgeHandshake(), [stopBridgeHandshake]);
  React.useEffect(() => {
    if (bridgeReady) post("SET_THEME", { theme: isDark ? "dark" : "light" });
  }, [bridgeReady, isDark, post]);
  React.useEffect(() => {
    const beforeUnload = (event: BeforeUnloadEvent) => {
      if (!stateRef.current.dirty) return;
      event.preventDefault();
      event.returnValue = "";
    };
    window.addEventListener("beforeunload", beforeUnload);
    return () => window.removeEventListener("beforeunload", beforeUnload);
  }, []);

  const moveHistory = React.useCallback(
    (direction: -1 | 1) => {
      const next = moveLifeNavigation(navigationRef.current, direction);
      const resource = currentLifeNavigationEntry(next);
      if (!resource || next === navigationRef.current) return;
      requestLeave(() => {
        setNavigation(next);
        commitNavigation(resource, "history");
      });
    },
    [commitNavigation, requestLeave],
  );

  const logout = React.useCallback(() => {
    manuallyDisconnectedRef.current = true;
    post("LOGOUT");
    const token = workbenchSessionTokenRef.current;
    const id = embedSessionIdRef.current;
    if (gateway && token && id) void revokeLifeEmbedSession(gateway, token, id);
    workbenchSessionTokenRef.current = null;
    setSessionExpiresAt(null);
    embedSessionIdRef.current = null;
    setAuth({ phase: "unconnected" });
  }, [gateway, post]);

  const value = React.useMemo<LifeDockContextValue>(
    () => ({
      active,
      auth,
      bridgeReady,
      canGoBack: canMoveLifeNavigation(navigation, -1),
      canGoForward: canMoveLifeNavigation(navigation, 1),
      canResetWidth: width.canReset,
      close: () => requestLeave(() => dispatch({ type: "close" })),
      config,
      configError:
        configResult.error ??
        (!gateway ? "Life Auth Gateway is not configured." : null),
      goBack: () => moveHistory(-1),
      goForward: () => moveHistory(1),
      goHome: () => homeResource && openLifeResource(homeResource),
      iframeRef,
      isOverlay: width.isOverlay,
      logout,
      onBrowserLoad: () => {
        dispatch({ type: "loading", loading: false });
        setBridgeReady(false);
        startBridgeHandshake();
      },
      onResetWidth: width.onResetWidth,
      onResizeStart: width.onResizeStart,
      openCurrentInBrowser: () => {
        const url =
          stateRef.current.currentResource && config
            ? buildLifeUrl(stateRef.current.currentResource, config)
            : null;
        if (url)
          void openUrl(url).catch(() => toast.error("Failed to open LifeOS"));
      },
      openLifeResource,
      openLifeResourceAutomatically: (resource) => {
        if (
          !canNavigateLifeResource({
            activeExtensionId,
            dirty: stateRef.current.dirty,
            followConversation: stateRef.current.followConversation,
            pinned: stateRef.current.pinned,
            source: "automatic",
          })
        )
          return false;
        commitNavigation(resource);
        return true;
      },
      openLifeResourceInBrowser: (resource) => {
        const url = config ? buildLifeUrl(resource, config) : null;
        if (url)
          void openUrl(url).catch(() => toast.error("Failed to open LifeOS"));
      },
      refresh: () => {
        dispatch({ type: "loading", loading: true });
        if (!bridgeReady || !post("REFRESH")) {
          const frame = iframeRef.current;
          if (frame && config)
            frame.src = stateRef.current.currentUrl || config.homeUrl;
        }
      },
      renderedWidthPx: width.renderedWidthPx,
      resolveLifeResourceLink: resolveLifeResource,
      startSession: () => {
        manuallyDisconnectedRef.current = false;
        recoveryAttemptsRef.current = 0;
        workbenchSessionTokenRef.current = null;
        setSessionExpiresAt(null);
        startLifeSession(false);
      },
      state,
      toggle: () => {
        if (stateRef.current.open && !active) {
          const decision = requestDockActivation?.("life");
          if (decision && !decision.allowed)
            toast.warning(
              "Save or discard changes in the active workspace first.",
            );
        } else if (stateRef.current.open)
          requestLeave(() => dispatch({ type: "close" }));
        else dispatch({ type: "open" });
      },
      toggleFollowConversation: () => dispatch({ type: "toggle-follow" }),
      toggleFullscreen: () =>
        stateRef.current.fullscreen
          ? requestLeave(() => dispatch({ type: "exit-fullscreen" }))
          : dispatch({ type: "toggle-fullscreen" }),
      togglePinned: () => dispatch({ type: "toggle-pinned" }),
      lifeAuthPhase: lifeAuth.phase,
    }),
    [
      active,
      activeExtensionId,
      auth,
      bridgeReady,
      commitNavigation,
      config,
      configResult.error,
      gateway,
      homeResource,
      logout,
      moveHistory,
      navigation,
      openLifeResource,
      post,
      requestDockActivation,
      requestLeave,
      startBridgeHandshake,
      startLifeSession,
      state,
      width,
      lifeAuth.phase,
    ],
  );

  return (
    <LifeDockContext.Provider value={value}>
      {children}
      <AlertDialog
        open={leaveConfirmationOpen}
        onOpenChange={(open) => {
          setLeaveConfirmationOpen(open);
          if (!open) pendingLeaveRef.current = null;
        }}
      >
        <AlertDialogContent data-testid="life-dock-dirty-dialog">
          <AlertDialogHeader>
            <AlertDialogTitle>
              当前 LifeOS 页面存在未保存更改。
            </AlertDialogTitle>
            <AlertDialogDescription>
              离开后这些修改可能丢失。
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>取消</AlertDialogCancel>
            <AlertDialogAction
              onClick={() => {
                const action = pendingLeaveRef.current;
                pendingLeaveRef.current = null;
                setLeaveConfirmationOpen(false);
                dispatch({ type: "dirty", dirty: false });
                action?.();
              }}
            >
              仍然离开
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </LifeDockContext.Provider>
  );
}

export function useLifeDock(): LifeDockContextValue {
  const context = React.useContext(LifeDockContext);
  if (!context)
    throw new Error("useLifeDock must be used within LifeDockProvider");
  return context;
}

export function useOptionalLifeDock(): LifeDockContextValue | null {
  return React.useContext(LifeDockContext);
}
