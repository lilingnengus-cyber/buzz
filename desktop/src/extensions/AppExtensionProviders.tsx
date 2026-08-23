import type * as React from "react";

import { BusinessIamAdminProvider } from "@/features/business-iam-admin";
import { BusinessDockProvider } from "@/features/business-dock";
import {
  WorkbenchAuthGate,
  WorkbenchAuthProvider,
} from "@/features/workbench-auth";

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
          <BusinessDockProvider>{children}</BusinessDockProvider>
        </BusinessIamAdminProvider>
      </WorkbenchAuthGate>
    </WorkbenchAuthProvider>
  );
}
