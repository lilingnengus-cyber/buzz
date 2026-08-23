import type * as React from "react";

import { AUXILIARY_PANEL_MIN_WIDTH_PX } from "@/shared/layout/AuxiliaryPanel";
import { cn } from "@/shared/lib/cn";

type RightAuxiliaryPaneProps = {
  canResetWidth: boolean;
  children: React.ReactNode;
  constrainToAvailableSpace?: boolean;
  detached?: boolean;
  onResetWidth: () => void;
  onResizeStart: (event: React.MouseEvent<HTMLButtonElement>) => void;
  testId?: string;
  widthPx: number;
};

export function RightAuxiliaryPane({
  canResetWidth,
  children,
  constrainToAvailableSpace = true,
  detached = false,
  onResetWidth,
  onResizeStart,
  testId,
  widthPx,
}: RightAuxiliaryPaneProps) {
  return (
    <aside
      className={cn(
        "group/right-pane relative flex h-full shrink-0 flex-col overflow-hidden bg-background",
        detached
          ? "bg-transparent"
          : "before:pointer-events-none before:absolute before:bottom-0 before:left-0 before:top-0 before:z-50 before:w-px before:bg-border/80 before:content-['']",
      )}
      data-testid={testId}
      style={{
        maxWidth: constrainToAvailableSpace
          ? `calc(100% - ${AUXILIARY_PANEL_MIN_WIDTH_PX}px)`
          : undefined,
        width: widthPx,
      }}
    >
      <button
        aria-label="Resize panel"
        className="peer/right-pane-resize group/right-pane-resize absolute inset-y-0 left-0 z-50 w-3 touch-none cursor-col-resize select-none"
        data-testid="right-auxiliary-pane-resize-handle"
        onDoubleClick={canResetWidth ? onResetWidth : undefined}
        onMouseDown={onResizeStart}
        title={
          canResetWidth
            ? "Drag to resize. Double-click to reset width."
            : "Drag to resize."
        }
        type="button"
      >
        <span className="absolute bottom-0 left-0 top-0 w-px bg-border/80 transition-[width,background-color] group-hover/right-pane-resize:w-1 group-hover/right-pane-resize:bg-primary/70 group-focus-visible/right-pane-resize:w-1 group-focus-visible/right-pane-resize:bg-primary/70" />
      </button>
      <div className="relative flex min-h-0 min-w-0 flex-1 flex-col">
        {children}
      </div>
    </aside>
  );
}
