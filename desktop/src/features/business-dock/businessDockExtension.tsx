import { BusinessDock } from "@/features/business-dock/BusinessDock";
import { BusinessDockProvider } from "@/features/business-dock/BusinessDockProvider";
import { BusinessDockTopChromeAction } from "@/features/business-dock/BusinessDockTopChromeAction";
import { getBusinessDockConfig } from "@/features/business-dock/businessDockConfig";
import { resolveBusinessResource } from "@/features/business-dock/businessResourceResolver";
import type { WorkspaceDockExtension } from "@/features/workspace-dock";
import type * as React from "react";

function BusinessDockExtensionProvider({ children }: React.PropsWithChildren) {
  return <BusinessDockProvider>{children}</BusinessDockProvider>;
}

export function createBusinessDockExtension(): WorkspaceDockExtension {
  const config = getBusinessDockConfig().config;
  return {
    id: "business",
    title: "Business system",
    scheme: "biz",
    origin: config?.origin ?? null,
    homeUrl: config?.homeUrl ?? null,
    resolveResource: (input) =>
      config ? resolveBusinessResource(input, config) : null,
    Provider: BusinessDockExtensionProvider,
    Dock: BusinessDock,
    TopChromeAction: BusinessDockTopChromeAction,
  };
}

export const businessDockExtension = createBusinessDockExtension();
