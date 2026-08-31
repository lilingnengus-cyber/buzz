import type * as React from "react";

import { BusinessIamAdminProvider } from "@/features/business-iam-admin";
import { businessDockExtension } from "@/features/business-dock";
import { WorkspaceDockHostProvider } from "@/features/workspace-dock";
import {
  WorkbenchAuthGate,
  WorkbenchAuthProvider,
} from "@/features/workbench-auth";

const APP_WORKSPACE_DOCK_EXTENSIONS = [businessDockExtension];

/** Product-specific providers kept behind one stable Buzz integration point. */
export function AppExtensionProviders({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <WorkbenchAuthProvider>
      <WorkbenchAuthGate>
        <BusinessIamAdminProvider>
          <WorkspaceDockHostProvider extensions={APP_WORKSPACE_DOCK_EXTENSIONS}>
            {children}
          </WorkspaceDockHostProvider>
        </BusinessIamAdminProvider>
      </WorkbenchAuthGate>
    </WorkbenchAuthProvider>
  );
}
