import * as React from "react";
import { HeartPulse } from "lucide-react";

import { useLifeDock } from "./LifeDockProvider";
import { cn } from "../../shared/lib/cn";
import { Button } from "../../shared/ui/button";

const CLASS_NAME =
  "h-[28px] w-[28px] rounded-[4px] text-sidebar-foreground/65 hover:bg-sidebar-accent hover:text-sidebar-accent-foreground";

export function LifeDockTopChromeAction() {
  const { active, state, toggle } = useLifeDock();
  React.useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (
        event.key.toLowerCase() !== "l" ||
        !event.shiftKey ||
        (!event.metaKey && !event.ctrlKey)
      )
        return;
      event.preventDefault();
      toggle();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [toggle]);
  return (
    <Button
      aria-label="Toggle Life Dock"
      aria-pressed={state.open && active}
      className={cn(
        CLASS_NAME,
        state.open &&
          active &&
          "bg-sidebar-accent text-sidebar-accent-foreground",
      )}
      data-testid="life-dock-toggle"
      onClick={toggle}
      size="icon"
      title="LifeOS (⌘⇧L / Ctrl+Shift+L)"
      type="button"
      variant="ghost"
    >
      <HeartPulse />
      <span className="sr-only">Toggle Life Dock</span>
    </Button>
  );
}
