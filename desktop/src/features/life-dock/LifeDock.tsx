import { LifeDockBrowser } from "./LifeDockBrowser";
import { useLifeDock } from "./LifeDockProvider";
import { LifeDockToolbar } from "./LifeDockToolbar";
import { LifeTrustedTurnFollower } from "./LifeTrustedTurnFollower";
import { cn } from "../../shared/lib/cn";

export function LifeDock() {
  const {
    active,
    canResetWidth,
    close,
    isOverlay,
    onResetWidth,
    onResizeStart,
    renderedWidthPx,
    state,
  } = useLifeDock();
  const floating = isOverlay || state.fullscreen;

  return (
    <>
      <LifeTrustedTurnFollower />
      {state.open &&
      active &&
      isOverlay &&
      !state.fullscreen &&
      !state.pinned ? (
        <button
          aria-label="Close Life Dock overlay"
          className="absolute inset-0 z-[110] bg-black/20 backdrop-blur-[1px]"
          onClick={close}
          type="button"
        />
      ) : null}
      <aside
        aria-hidden={!state.open || !active}
        className={cn(
          "z-[120] flex min-h-0 flex-col overflow-hidden bg-background",
          state.open && active ? "visible" : "invisible pointer-events-none",
          floating
            ? "absolute inset-y-0 right-0 shadow-2xl"
            : "relative shrink-0 border-l border-border/70",
          state.fullscreen && "left-0 w-full! shadow-none",
        )}
        data-life-dock-active={active}
        data-life-dock-open={state.open}
        data-testid="life-dock"
        style={{
          width:
            state.open && active
              ? state.fullscreen
                ? "100%"
                : renderedWidthPx
              : 0,
        }}
      >
        {state.open && active && !floating ? (
          <button
            aria-label="Resize Life Dock"
            className="group absolute inset-y-0 left-0 z-50 w-3 touch-none cursor-col-resize select-none"
            data-testid="life-dock-resize-handle"
            onDoubleClick={canResetWidth ? onResetWidth : undefined}
            onMouseDown={onResizeStart}
            title={
              canResetWidth
                ? "Drag to resize. Double-click to reset width."
                : "Drag to resize."
            }
            type="button"
          >
            <span className="absolute inset-y-0 left-0 w-px bg-border/80 transition-[width,background-color] group-hover:w-1 group-hover:bg-primary/70 group-focus-visible:w-1 group-focus-visible:bg-primary/70" />
          </button>
        ) : null}
        <LifeDockToolbar />
        {state.browserMounted ? <LifeDockBrowser /> : null}
      </aside>
    </>
  );
}
