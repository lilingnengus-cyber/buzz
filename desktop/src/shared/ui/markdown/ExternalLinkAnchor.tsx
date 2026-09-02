import * as React from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { toast } from "sonner";

import { useOptionalBusinessDock } from "@/features/business-dock/BusinessDockProvider";
import { buildBusinessUrl } from "@/features/business-dock/businessResourceResolver";
import { useOptionalLifeDock } from "@/features/life-dock";
import { lifeLinkAction } from "@/features/life-dock/lifeLinkHandler";
import { buildLifeUrl } from "@/features/life-dock/lifeResourceResolver";
import { cn } from "@/shared/lib/cn";
import { copyTextToClipboard } from "@/shared/lib/clipboard";

import { MaskedLinkTooltip } from "./MaskedLinkTooltip";
import {
  MediaContextMenu,
  type MediaContextMenuPosition,
  useDismissMediaContextMenu,
} from "./MediaContextMenu";

/**
 * An external `[text](href)` link with a custom right-click menu.
 *
 * Buzz renders inside a native webview whose default context menu has no
 * useful link actions, so a plain right-click on a link is a no-op. This adds
 * an in-app menu with "Open link" (via the OS opener, matching the anchor's
 * left-click `target="_blank"` behavior) and "Copy link" (the real href, not
 * the masked display text).
 */
export function ExternalLinkAnchor({
  anchorProps,
  children,
  href,
  isLinearLink,
  label,
}: {
  anchorProps: React.ComponentPropsWithoutRef<"a">;
  children: React.ReactNode;
  href: string | undefined;
  isLinearLink: boolean;
  label: string;
}) {
  const businessDock = useOptionalBusinessDock();
  const lifeDock = useOptionalLifeDock();
  const businessResource =
    href && businessDock
      ? businessDock.resolveBusinessResourceLink(href)
      : null;
  const businessLink =
    businessResource &&
    businessDock?.config &&
    buildBusinessUrl(businessResource, businessDock.config)
      ? {
          onOpenInBrowser: () =>
            businessDock.openBusinessResourceInBrowser(businessResource),
          onOpenInDock: () =>
            businessDock.openBusinessResource(businessResource),
        }
      : null;
  const lifeAction = lifeLinkAction(href, false);
  const lifeLink =
    lifeAction &&
    lifeDock?.config &&
    buildLifeUrl(lifeAction.resource, lifeDock.config)
      ? {
          onOpenInBrowser: () =>
            lifeDock.openLifeResourceInBrowser(lifeAction.resource),
          onOpenInDock: () => lifeDock.openLifeResource(lifeAction.resource),
        }
      : null;
  const workspaceLink = businessLink
    ? { ...businessLink, dockLabel: "Open in Business Dock" }
    : lifeLink
      ? { ...lifeLink, dockLabel: "Open in Life Dock" }
      : null;
  const [menu, setMenu] = React.useState<MediaContextMenuPosition | null>(null);
  const closeMenu = React.useCallback(() => setMenu(null), []);
  useDismissMediaContextMenu(Boolean(menu), closeMenu);

  const anchor = (
    <a
      {...anchorProps}
      className={cn(
        "font-medium underline underline-offset-4 transition-colors",
        isLinearLink ? "linear-link" : "text-primary hover:text-primary/80",
      )}
      href={href}
      onClick={(event) => {
        if (!workspaceLink) {
          anchorProps.onClick?.(event);
          return;
        }
        event.preventDefault();
        if (event.metaKey || event.ctrlKey) {
          workspaceLink.onOpenInBrowser();
          return;
        }
        workspaceLink.onOpenInDock();
      }}
      onContextMenuCapture={(event) => {
        if (!href) return;
        event.preventDefault();
        setMenu({ x: event.clientX, y: event.clientY });
      }}
      rel="noreferrer"
      target="_blank"
    >
      {children}
    </a>
  );

  return (
    <>
      <MaskedLinkTooltip disabled={isLinearLink} href={href} label={label}>
        {anchor}
      </MaskedLinkTooltip>
      {menu && href ? (
        <MediaContextMenu
          dataAttributes={["data-link-context-menu"]}
          items={[
            ...(workspaceLink
              ? [
                  {
                    label: workspaceLink.dockLabel,
                    onSelect: () => {
                      closeMenu();
                      workspaceLink.onOpenInDock();
                    },
                  },
                ]
              : []),
            {
              label: workspaceLink ? "Open in Browser" : "Open link",
              onSelect: () => {
                closeMenu();
                if (workspaceLink) {
                  workspaceLink.onOpenInBrowser();
                  return;
                }
                void openUrl(href).catch(() => {
                  toast.error("Failed to open link");
                });
              },
            },
            {
              label: "Copy link",
              onSelect: () => {
                closeMenu();
                copyTextToClipboard(href, "Link copied to clipboard");
              },
            },
          ]}
          position={menu}
        />
      ) : null}
    </>
  );
}
