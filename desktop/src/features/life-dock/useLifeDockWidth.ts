import * as React from "react";

import {
  formatHorizontalResizeIndicator,
  startHorizontalMouseResize,
} from "@/shared/lib/startHorizontalMouseResize";

const DEFAULT_WIDTH = 560;
const MIN_WIDTH = 420;
const OVERLAY_BREAKPOINT = 1000;
const SESSION_KEY = "buzz.life-dock.width";

function viewportWidth(): number {
  return typeof window === "undefined" ? 0 : window.innerWidth;
}

function clamp(width: number, viewport: number): number {
  const maximum =
    viewport < OVERLAY_BREAKPOINT ? viewport : Math.floor(viewport * 0.5);
  return Math.max(MIN_WIDTH, Math.min(Math.max(MIN_WIDTH, maximum), width));
}

function initialWidth(): number {
  if (typeof window === "undefined") return DEFAULT_WIDTH;
  try {
    const stored = Number.parseInt(
      window.sessionStorage.getItem(SESSION_KEY) ?? "",
      10,
    );
    return Number.isFinite(stored)
      ? clamp(stored, viewportWidth())
      : DEFAULT_WIDTH;
  } catch {
    return DEFAULT_WIDTH;
  }
}

export function useLifeDockWidth() {
  const [width, setWidth] = React.useState(initialWidth);
  const [viewport, setViewport] = React.useState(viewportWidth);
  React.useEffect(() => {
    const resize = () => setViewport(viewportWidth());
    window.addEventListener("resize", resize);
    return () => window.removeEventListener("resize", resize);
  }, []);
  React.useEffect(() => {
    try {
      window.sessionStorage.setItem(SESSION_KEY, String(width));
    } catch {
      // Width persistence is optional.
    }
  }, [width]);
  const onResizeStart = React.useCallback(
    (event: React.MouseEvent<HTMLButtonElement>) => {
      if (event.detail > 1) return;
      const startX = event.clientX;
      const startWidth = width;
      startHorizontalMouseResize(
        event,
        (clientX) => {
          const currentViewport = viewportWidth();
          const next = clamp(startWidth + startX - clientX, currentViewport);
          setWidth(next);
          return formatHorizontalResizeIndicator(next, currentViewport);
        },
        {
          indicatorText: formatHorizontalResizeIndicator(
            width,
            viewportWidth(),
          ),
        },
      );
    },
    [width],
  );
  return {
    canReset: width !== DEFAULT_WIDTH,
    isOverlay: viewport < OVERLAY_BREAKPOINT,
    onResetWidth: () => setWidth(DEFAULT_WIDTH),
    onResizeStart,
    renderedWidthPx:
      viewport < OVERLAY_BREAKPOINT
        ? Math.min(width, viewport)
        : clamp(width, viewport),
  };
}
