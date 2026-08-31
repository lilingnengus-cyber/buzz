import { BusinessIamAdminTopChromeAction } from "@/features/business-iam-admin";
import { WorkspaceDockTopChromeActions } from "@/features/workspace-dock";

/** Product-specific chrome actions kept behind one stable Buzz integration point. */
export function AppExtensionTopChromeActions() {
  return (
    <>
      <BusinessIamAdminTopChromeAction />
      <WorkspaceDockTopChromeActions />
    </>
  );
}
