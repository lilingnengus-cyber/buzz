import { Fragment, type ReactNode } from "react";
import { Check, Circle, LockKeyhole, UserRound } from "lucide-react";

import type { IamChangeRequest } from "@/features/business-iam-admin/businessIamAdminApi";
import { cn } from "@/shared/lib/cn";

export function ApprovalRail({ change }: { change: IamChangeRequest }) {
  const reviewSlots = Array.from(
    { length: change.requiredApprovals },
    (_, index) =>
      change.approvals.filter((item) => item.decision === "approve")[index],
  );
  const applied = change.status === "applied";
  const nodes = [
    {
      complete: true,
      detail: change.requesterDisplayName,
      icon: <UserRound className="size-4" />,
      label: "Requested",
    },
    ...reviewSlots.map((approval, index) => ({
      complete: Boolean(approval),
      detail: approval?.approverDisplayName ?? "Reviewer",
      icon: approval ? (
        <Check className="size-4" />
      ) : (
        <Circle className="size-4" />
      ),
      label: `Review ${index + 1}`,
    })),
    {
      complete: applied,
      detail: applied ? "Policy applied" : "No authority changed",
      icon: <LockKeyhole className="size-4" />,
      label: "Effective",
    },
  ];

  return (
    <fieldset
      aria-label={`${change.approvalCount} of ${change.requiredApprovals} approvals complete`}
      className="overflow-x-auto pb-1"
      data-testid="iam-approval-rail"
    >
      <div className="flex min-w-max items-start justify-center">
        {nodes.map((node, index) => (
          <Fragment key={`${change.id}-${node.label}`}>
            <RailNode {...node} />
            {index < nodes.length - 1 ? (
              <RailLine complete={nodes[index + 1].complete} />
            ) : null}
          </Fragment>
        ))}
      </div>
    </fieldset>
  );
}

function RailLine({ complete }: { complete: boolean }) {
  return (
    <div
      className={cn(
        "mt-5 h-px w-10 shrink-0 bg-border transition-colors",
        complete && "bg-emerald-500/70",
      )}
    />
  );
}

function RailNode({
  complete,
  detail,
  icon,
  label,
}: {
  complete: boolean;
  detail: string;
  icon: ReactNode;
  label: string;
}) {
  return (
    <div className="w-32 shrink-0 text-center">
      <div
        className={cn(
          "mx-auto grid size-10 place-items-center rounded-full border bg-background text-muted-foreground",
          complete &&
            "border-emerald-500/40 bg-emerald-500/10 text-emerald-600 dark:text-emerald-400",
        )}
      >
        {icon}
      </div>
      <p className="mt-2 text-xs font-semibold text-foreground">{label}</p>
      <p className="mt-0.5 truncate text-2xs text-muted-foreground">{detail}</p>
    </div>
  );
}
