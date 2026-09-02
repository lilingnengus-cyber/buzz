import type * as React from "react";

import { LifeDock } from "./LifeDock";
import { LifeDockProvider } from "./LifeDockProvider";
import { LifeDockTopChromeAction } from "./LifeDockTopChromeAction";
import { getLifeDockConfig } from "./lifeDockConfig";
import { resolveLifeResource } from "./lifeResourceResolver";
import type { WorkspaceDockExtension } from "../workspace-dock";

function Provider({ children }: React.PropsWithChildren) {
  return <LifeDockProvider>{children}</LifeDockProvider>;
}

export function createLifeDockExtension(): WorkspaceDockExtension | null {
  const result = getLifeDockConfig();
  if (!result.enabled || !result.config) return null;
  return {
    id: "life",
    title: "LifeOS personal workspace",
    scheme: "life",
    origin: result.config.origin,
    homeUrl: result.config.homeUrl,
    resolveResource: resolveLifeResource,
    Provider,
    Dock: LifeDock,
    TopChromeAction: LifeDockTopChromeAction,
  };
}

export const lifeDockExtension = createLifeDockExtension();
