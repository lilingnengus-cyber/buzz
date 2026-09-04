import type * as React from "react";

import { BusinessIamAdminProvider } from "@/features/business-iam-admin";
import { businessDockExtension } from "@/features/business-dock";
import { lifeDockExtension } from "@/features/life-dock";
import { LifeAuthProvider } from "@/features/life-auth";
import { WorkspaceDockHostProvider } from "@/features/workspace-dock";
import {
  WorkbenchAuthGate,
  WorkbenchAuthProvider,
} from "@/features/workbench-auth";

const APP_WORKSPACE_DOCK_EXTENSIONS = [
  businessDockExtension,
  ...(lifeDockExtension ? [lifeDockExtension] : []),
];

/** Product-specific providers kept behind one stable Buzz integration point. */
export function AppExtensionProviders({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <WorkbenchAuthProvider>
      <WorkbenchAuthGate>
        <LifeAuthProvider>
          <BusinessIamAdminProvider>
            <WorkspaceDockHostProvider
              extensions={APP_WORKSPACE_DOCK_EXTENSIONS}
            >
              {children}
            </WorkspaceDockHostProvider>
          </BusinessIamAdminProvider>
        </LifeAuthProvider>
      </WorkbenchAuthGate>
    </WorkbenchAuthProvider>
  );
}
