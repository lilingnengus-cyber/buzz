import {
  ArrowLeft,
  ArrowRight,
  Copy,
  Eye,
  EyeOff,
  ExternalLink,
  Home,
  LogOut,
  Maximize2,
  Minimize2,
  Pin,
  RotateCw,
  X,
} from "lucide-react";

import { useLifeDock } from "./LifeDockProvider";
import {
  buildLifeReference,
  formatLifeResourceLabel,
} from "./lifeResourceResolver";
import { copyTextToClipboard } from "../../shared/lib/clipboard";
import { cn } from "../../shared/lib/cn";
import { Button } from "../../shared/ui/button";

const TOOLBAR_BUTTON_CLASS = "h-8 w-8 shrink-0";

export function LifeDockToolbar() {
  const {
    auth,
    canGoBack,
    canGoForward,
    close,
    goBack,
    goForward,
    goHome,
    logout,
    openCurrentInBrowser,
    refresh,
    state,
    toggleFollowConversation,
    toggleFullscreen,
    togglePinned,
  } = useLifeDock();
  const label = state.currentResource
    ? formatLifeResourceLabel(state.currentResource)
    : state.title || "LifeOS";
  const controls = [
    { label: "LifeOS home", icon: Home, action: goHome, disabled: false },
    {
      label: "LifeOS back",
      icon: ArrowLeft,
      action: goBack,
      disabled: !canGoBack,
    },
    {
      label: "LifeOS forward",
      icon: ArrowRight,
      action: goForward,
      disabled: !canGoForward,
    },
    {
      label: "Refresh LifeOS",
      icon: RotateCw,
      action: refresh,
      disabled: false,
    },
  ];
  return (
    <div
      className="flex h-11 shrink-0 cursor-default select-none items-center gap-0.5 border-b border-border/50 bg-background/90 px-2 backdrop-blur-md"
      data-testid="life-dock-toolbar"
    >
      {controls.map(({ action, disabled, icon: Icon, label: controlLabel }) => (
        <Button
          aria-label={controlLabel}
          className={TOOLBAR_BUTTON_CLASS}
          disabled={disabled}
          key={controlLabel}
          onClick={action}
          size="icon"
          title={controlLabel}
          variant="ghost"
        >
          <Icon />
        </Button>
      ))}
      <div className="min-w-0 flex-1 px-2 text-center">
        <p
          className="flex items-center justify-center gap-1.5 truncate text-sm font-medium"
          title={label}
        >
          {state.dirty ? (
            <span
              aria-hidden="true"
              className="size-1.5 shrink-0 rounded-full bg-amber-500"
              data-testid="life-dock-dirty-indicator"
            />
          ) : null}
          <span className="truncate" data-testid="life-resource-label">
            {label}
          </span>
        </p>
      </div>
      <Button
        aria-label={
          state.followConversation
            ? "Stop following LifeOS resources"
            : "Follow LifeOS resources"
        }
        aria-pressed={state.followConversation}
        className={cn(
          TOOLBAR_BUTTON_CLASS,
          state.followConversation && "text-primary",
        )}
        onClick={toggleFollowConversation}
        size="icon"
        variant="ghost"
      >
        {state.followConversation ? <Eye /> : <EyeOff />}
      </Button>
      <Button
        aria-label={state.pinned ? "Unpin Life Dock" : "Pin Life Dock"}
        aria-pressed={state.pinned}
        className={cn(TOOLBAR_BUTTON_CLASS, state.pinned && "text-primary")}
        onClick={togglePinned}
        size="icon"
        variant="ghost"
      >
        <Pin />
      </Button>
      <Button
        aria-label={
          state.fullscreen ? "Exit full screen LifeOS" : "Full screen LifeOS"
        }
        className={TOOLBAR_BUTTON_CLASS}
        onClick={toggleFullscreen}
        size="icon"
        variant="ghost"
      >
        {state.fullscreen ? <Minimize2 /> : <Maximize2 />}
      </Button>
      <Button
        aria-label="Copy LifeOS reference"
        className={TOOLBAR_BUTTON_CLASS}
        disabled={!state.currentResource}
        onClick={() => {
          const reference = state.currentResource
            ? buildLifeReference(state.currentResource)
            : null;
          if (reference)
            copyTextToClipboard(reference, "LifeOS reference copied");
        }}
        size="icon"
        variant="ghost"
      >
        <Copy />
      </Button>
      <Button
        aria-label="Open LifeOS in browser"
        className={TOOLBAR_BUTTON_CLASS}
        onClick={openCurrentInBrowser}
        size="icon"
        variant="ghost"
      >
        <ExternalLink />
      </Button>
      <Button
        aria-label="Sign out of LifeOS Dock"
        className={TOOLBAR_BUTTON_CLASS}
        disabled={auth.phase !== "authenticated"}
        onClick={logout}
        size="icon"
        variant="ghost"
      >
        <LogOut />
      </Button>
      <Button
        aria-label="Close Life Dock"
        className={TOOLBAR_BUTTON_CLASS}
        onClick={close}
        size="icon"
        variant="ghost"
      >
        <X />
      </Button>
    </div>
  );
}
