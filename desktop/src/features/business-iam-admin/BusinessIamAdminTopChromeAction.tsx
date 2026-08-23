import { ShieldCheck } from "lucide-react";

import { useBusinessIamAdmin } from "@/features/business-iam-admin/BusinessIamAdminProvider";
import { Button } from "@/shared/ui/button";
import { cn } from "@/shared/lib/cn";

const TOP_CHROME_ACTION_CLASS =
  "h-[28px] w-[28px] rounded-[4px] text-sidebar-foreground/65 hover:bg-sidebar-accent hover:text-sidebar-accent-foreground";

export function BusinessIamAdminTopChromeAction() {
  const { open, toggle } = useBusinessIamAdmin();
  return (
    <Button
      aria-label="Open authority ledger"
      aria-pressed={open}
      className={cn(
        TOP_CHROME_ACTION_CLASS,
        open && "bg-sidebar-accent text-sidebar-accent-foreground",
      )}
      data-testid="business-iam-admin-toggle"
      onClick={toggle}
      size="icon"
      title="Authority ledger"
      type="button"
      variant="ghost"
    >
      <ShieldCheck />
      <span className="sr-only">Open authority ledger</span>
    </Button>
  );
}
