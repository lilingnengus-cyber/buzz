import { BusinessDockTopChromeAction } from "@/features/business-dock/BusinessDockTopChromeAction";
import { BusinessIamAdminTopChromeAction } from "@/features/business-iam-admin";

/** Product-specific chrome actions kept behind one stable Buzz integration point. */
export function AppExtensionTopChromeActions() {
  return (
    <>
      <BusinessIamAdminTopChromeAction />
      <BusinessDockTopChromeAction />
    </>
  );
}
