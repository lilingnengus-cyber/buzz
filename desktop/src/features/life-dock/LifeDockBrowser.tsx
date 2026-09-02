import { ExternalLink, LoaderCircle, RotateCw } from "lucide-react";

import { useLifeDock } from "./LifeDockProvider";
import { Button } from "../../shared/ui/button";

export function LifeDockBrowser() {
  const {
    active,
    auth,
    config,
    configError,
    iframeRef,
    onBrowserLoad,
    startSession,
    state,
    workbenchAuthPhase,
  } = useLifeDock();

  if (!config) {
    return (
      <div
        className="flex min-h-0 flex-1 items-center justify-center px-8 text-center text-sm text-muted-foreground"
        data-testid="life-dock-unconfigured"
      >
        {configError ?? "LifeOS is not configured."}
      </div>
    );
  }

  const needsAuth =
    auth.phase === "unconnected" ||
    auth.phase === "expired" ||
    auth.phase === "failed";
  return (
    <div className="relative min-h-0 flex-1 bg-background">
      <iframe
        ref={iframeRef}
        allow="clipboard-read; clipboard-write"
        className="h-full w-full border-0 bg-background"
        data-testid="life-dock-iframe"
        onLoad={onBrowserLoad}
        referrerPolicy="no-referrer"
        sandbox="allow-downloads allow-forms allow-modals allow-popups allow-popups-to-escape-sandbox allow-same-origin allow-scripts"
        src={state.frameUrl}
        tabIndex={state.open && active ? 0 : -1}
        title={state.title ?? "LifeOS personal workspace"}
      />
      {state.loading ? (
        <div
          className="pointer-events-none absolute inset-0 flex items-center justify-center bg-background/60"
          data-testid="life-dock-loading"
        >
          <LoaderCircle className="size-5 animate-spin text-muted-foreground" />
          <span className="ml-2 text-sm text-muted-foreground">
            正在打开 LifeOS…
          </span>
        </div>
      ) : null}
      {needsAuth ? (
        <div
          className="absolute inset-x-4 bottom-4 rounded-lg border bg-background/95 p-4 shadow-lg backdrop-blur"
          data-testid="life-auth-required"
        >
          <p className="text-sm font-medium">
            {auth.phase === "expired"
              ? "LifeOS session expired"
              : auth.phase === "failed"
                ? "LifeOS connection failed"
                : "Connect LifeOS"}
          </p>
          <p className="mt-1 text-xs text-muted-foreground">
            {auth.reason ??
              "Create an isolated Life Dock session from your Workbench identity."}
          </p>
          <Button className="mt-3" onClick={startSession} size="sm">
            {workbenchAuthPhase === "authenticated" ? (
              <RotateCw />
            ) : (
              <ExternalLink />
            )}
            {workbenchAuthPhase === "authenticated"
              ? "Connect again"
              : "Sign in to Workbench"}
          </Button>
        </div>
      ) : null}
    </div>
  );
}
