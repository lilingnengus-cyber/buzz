import { Clock3, ShieldAlert } from "lucide-react";

import type { IamChangeRequest } from "@/features/business-iam-admin/businessIamAdminApi";
import { cn } from "@/shared/lib/cn";
import { Badge } from "@/shared/ui/badge";

const OPERATION_LABELS: Record<IamChangeRequest["operation"], string> = {
  principal_upsert: "Add or restore principal",
  principal_disable: "Disable principal",
  role_upsert: "Add or restore role",
  role_disable: "Disable role",
  permission_grant: "Grant direct permission",
  permission_revoke: "Revoke direct permission",
  role_permission_grant: "Add permission to role",
  role_permission_revoke: "Remove permission from role",
  role_assign: "Assign role",
  role_unassign: "Remove role",
};

export function operationLabel(operation: IamChangeRequest["operation"]) {
  return OPERATION_LABELS[operation];
}

export function ChangeQueue({
  changes,
  selectedId,
  onSelect,
}: {
  changes: IamChangeRequest[];
  selectedId: string | null;
  onSelect: (id: string) => void;
}) {
  if (changes.length === 0)
    return (
      <div className="grid min-h-48 place-items-center rounded-xl border border-dashed p-6 text-center">
        <div>
          <ShieldAlert className="mx-auto size-5 text-muted-foreground" />
          <p className="mt-3 text-sm font-medium">No changes in this queue</p>
          <p className="mt-1 text-xs text-muted-foreground">
            New requests appear here before authority changes.
          </p>
        </div>
      </div>
    );
  return (
    <div className="space-y-2" data-testid="iam-change-queue">
      {changes.map((change) => (
        <button
          className={cn(
            "w-full rounded-xl border bg-card px-3 py-3 text-left transition-colors hover:bg-accent/50 focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-ring",
            selectedId === change.id && "border-primary/40 bg-primary/5",
          )}
          key={change.id}
          onClick={() => onSelect(change.id)}
          type="button"
        >
          <div className="flex items-center justify-between gap-2">
            <Badge
              variant={
                change.riskLevel === "critical" ? "destructive" : "warning"
              }
            >
              {change.riskLevel}
            </Badge>
            <span className="flex items-center gap-1 text-2xs text-muted-foreground">
              <Clock3 className="size-3" />
              {new Date(change.requestedAt).toLocaleDateString()}
            </span>
          </div>
          <p className="mt-2 text-sm font-semibold">
            {operationLabel(change.operation)}
          </p>
          <p className="mt-1 line-clamp-2 text-xs text-muted-foreground">
            {change.reason}
          </p>
          <p className="mt-2 text-2xs text-muted-foreground">
            {change.approvalCount}/{change.requiredApprovals} reviews ·{" "}
            {change.requesterDisplayName}
          </p>
        </button>
      ))}
    </div>
  );
}
