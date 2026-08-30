import * as React from "react";
import {
  Check,
  FileClock,
  RefreshCw,
  ShieldCheck,
  ShieldX,
} from "lucide-react";

import { ApprovalRail } from "@/features/business-iam-admin/ApprovalRail";
import { AuthorityCatalog } from "@/features/business-iam-admin/AuthorityCatalog";
import {
  ChangeQueue,
  operationLabel,
} from "@/features/business-iam-admin/ChangeQueue";
import { ChangeRequestForm } from "@/features/business-iam-admin/ChangeRequestForm";
import {
  createIamChange,
  decideIamChange,
  describeIamError,
  type IamCatalog,
  type IamChangeRequest,
  readIamCatalog,
  readIamChanges,
} from "@/features/business-iam-admin/businessIamAdminApi";
import { getBusinessIamAdminConfig } from "@/features/business-iam-admin/businessIamAdminConfig";
import { useWorkbenchAuth } from "@/features/workbench-auth";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/shared/ui/tabs";
import { Textarea } from "@/shared/ui/textarea";

type View = "review" | "catalog" | "request";

export function BusinessIamAdminDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const configResult = React.useMemo(getBusinessIamAdminConfig, []);
  const workbenchAuth = useWorkbenchAuth();
  const [catalog, setCatalog] = React.useState<IamCatalog | null>(null);
  const [changes, setChanges] = React.useState<IamChangeRequest[]>([]);
  const [selectedId, setSelectedId] = React.useState<string | null>(null);
  const [view, setView] = React.useState<View>("review");
  const [historyVisible, setHistoryVisible] = React.useState(false);
  const [loading, setLoading] = React.useState(false);
  const [mutating, setMutating] = React.useState(false);
  const [error, setError] = React.useState<string | null>(configResult.error);
  const [comment, setComment] = React.useState("");

  const load = React.useCallback(async () => {
    if (!configResult.config) return;
    setLoading(true);
    setError(null);
    try {
      const token = await workbenchAuth.getAccessToken();
      if (!token)
        throw new Error("Sign in to Workbench to open authority controls.");
      const [nextCatalog, nextChanges] = await Promise.all([
        readIamCatalog(configResult.config.baseUrl, token),
        readIamChanges(configResult.config.baseUrl, token),
      ]);
      setCatalog(nextCatalog);
      setChanges(nextChanges);
      setSelectedId((current) =>
        nextChanges.some((change) => change.id === current)
          ? current
          : (nextChanges.find((change) => change.status === "pending")?.id ??
            nextChanges[0]?.id ??
            null),
      );
    } catch (cause) {
      setError(describeIamError(cause));
    } finally {
      setLoading(false);
    }
  }, [configResult.config, workbenchAuth.getAccessToken]);

  React.useEffect(() => {
    if (open) void load();
  }, [load, open]);

  const selected = changes.find((change) => change.id === selectedId) ?? null;
  const visibleChanges = historyVisible
    ? changes
    : changes.filter((change) => change.status === "pending");

  const decide = async (decision: "approve" | "reject") => {
    if (!selected || !configResult.config) return;
    if (comment.trim().length < 3) {
      setError("Add a short review comment before deciding.");
      return;
    }
    setMutating(true);
    setError(null);
    try {
      const token = await workbenchAuth.getAccessToken();
      if (!token) throw new Error("Workbench session expired.");
      const updated = await decideIamChange(
        configResult.config.baseUrl,
        token,
        selected.id,
        decision,
        comment.trim(),
      );
      setChanges((current) =>
        current.map((change) => (change.id === updated.id ? updated : change)),
      );
      setComment("");
      await load();
    } catch (cause) {
      setError(describeIamError(cause));
    } finally {
      setMutating(false);
    }
  };

  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogContent
        className="flex h-[min(48rem,calc(100vh-2rem))] max-w-[72rem] flex-col gap-0 overflow-hidden p-0"
        data-testid="business-iam-admin-dialog"
      >
        <header className="flex items-start justify-between gap-5 border-b px-6 py-5 pr-14">
          <DialogHeader>
            <div className="flex items-center gap-3">
              <div className="grid size-10 place-items-center rounded-xl bg-primary/10 text-primary">
                <ShieldCheck className="size-5" />
              </div>
              <div>
                <DialogTitle>Authority ledger</DialogTitle>
                <DialogDescription>
                  Human and Agent access changes, separated from Buzz
                  membership.
                </DialogDescription>
              </div>
            </div>
          </DialogHeader>
          <div className="flex items-center gap-2">
            <Button
              aria-label="Refresh authority ledger"
              disabled={loading}
              onClick={() => void load()}
              size="icon"
              type="button"
              variant="ghost"
            >
              <RefreshCw
                className={loading ? "size-4 animate-spin" : "size-4"}
              />
            </Button>
          </div>
        </header>

        {!configResult.config ? (
          <EmptyState
            detail={
              configResult.error ??
              "Set VITE_BUSINESS_IAM_ADMIN_URL to connect the management plane."
            }
            title="Authority ledger is not configured"
          />
        ) : (
          <Tabs
            className="flex min-h-0 flex-1 flex-col"
            onValueChange={(value) => setView(value as View)}
            value={view}
          >
            <div className="flex items-center justify-between border-b px-6 py-2.5">
              <TabsList>
                <TabsTrigger value="review">Review queue</TabsTrigger>
                <TabsTrigger value="catalog">Authority catalog</TabsTrigger>
                <TabsTrigger value="request">New request</TabsTrigger>
              </TabsList>
              {error ? (
                <p className="text-xs text-destructive">{error}</p>
              ) : null}
            </div>

            <TabsContent className="mt-0 min-h-0 flex-1" value="review">
              <div className="grid h-full min-h-0 grid-cols-[19rem_minmax(0,1fr)]">
                <aside className="min-h-0 overflow-auto border-r bg-muted/20 p-4">
                  <div className="mb-3 flex items-center justify-between">
                    <p className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">
                      {historyVisible ? "All changes" : "Needs review"}
                    </p>
                    <Button
                      onClick={() => setHistoryVisible((current) => !current)}
                      size="sm"
                      type="button"
                      variant="ghost"
                    >
                      {historyVisible ? "Pending" : "History"}
                    </Button>
                  </div>
                  <ChangeQueue
                    changes={visibleChanges}
                    onSelect={setSelectedId}
                    selectedId={selectedId}
                  />
                </aside>
                <main className="min-h-0 overflow-auto p-6">
                  {selected ? (
                    <ChangeReview
                      busy={mutating}
                      change={selected}
                      comment={comment}
                      onCommentChange={setComment}
                      onDecide={decide}
                    />
                  ) : (
                    <EmptyState
                      detail="Choose a change from the queue, or create a new request."
                      title="No change selected"
                    />
                  )}
                </main>
              </div>
            </TabsContent>
            <TabsContent
              className="mt-0 flex min-h-0 flex-1 p-6"
              value="catalog"
            >
              {catalog ? (
                <AuthorityCatalog catalog={catalog} />
              ) : (
                <EmptyState
                  detail="Refresh the ledger to retry."
                  title="Catalog unavailable"
                />
              )}
            </TabsContent>
            <TabsContent
              className="mt-0 flex min-h-0 flex-1 p-6"
              value="request"
            >
              {catalog ? (
                <ChangeRequestForm
                  busy={mutating}
                  catalog={catalog}
                  onCreate={async (input) => {
                    setMutating(true);
                    setError(null);
                    try {
                      const token = await workbenchAuth.getAccessToken();
                      if (!token) throw new Error("Workbench session expired.");
                      const created = await createIamChange(
                        configResult.config.baseUrl,
                        token,
                        {
                          ...input,
                          idempotencyKey: crypto.randomUUID(),
                        },
                      );
                      setChanges((current) => [created, ...current]);
                      setSelectedId(created.id);
                      setView("review");
                      setHistoryVisible(false);
                    } catch (cause) {
                      throw new Error(describeIamError(cause));
                    } finally {
                      setMutating(false);
                    }
                  }}
                />
              ) : (
                <EmptyState
                  detail="Refresh the ledger to retry."
                  title="Catalog unavailable"
                />
              )}
            </TabsContent>
          </Tabs>
        )}
      </DialogContent>
    </Dialog>
  );
}

function ChangeReview({
  busy,
  change,
  comment,
  onCommentChange,
  onDecide,
}: {
  busy: boolean;
  change: IamChangeRequest;
  comment: string;
  onCommentChange: (value: string) => void;
  onDecide: (decision: "approve" | "reject") => Promise<void>;
}) {
  return (
    <div className="mx-auto max-w-3xl" data-testid="iam-change-review">
      <div className="flex items-start justify-between gap-4">
        <div>
          <div className="flex items-center gap-2">
            <Badge
              variant={
                change.riskLevel === "critical" ? "destructive" : "warning"
              }
            >
              {change.riskLevel}
            </Badge>
            <Badge
              variant={change.status === "applied" ? "success" : "outline"}
            >
              {change.status}
            </Badge>
          </div>
          <h2 className="mt-3 text-xl font-semibold">
            {operationLabel(change.operation)}
          </h2>
          <p className="mt-1 text-sm text-muted-foreground">{change.reason}</p>
        </div>
        <p className="font-mono text-2xs text-muted-foreground">
          {change.traceId.slice(0, 8)}
        </p>
      </div>

      <section className="mt-6 rounded-2xl border bg-card p-5">
        <p className="mb-5 text-xs font-semibold uppercase tracking-wider text-muted-foreground">
          Approval flow
        </p>
        <ApprovalRail change={change} />
      </section>

      <section className="mt-4 grid gap-4 rounded-2xl border p-5 sm:grid-cols-2">
        {Object.entries(change.payload).map(([key, value]) => (
          <div key={key}>
            <p className="text-2xs font-semibold uppercase tracking-wider text-muted-foreground">
              {key.replaceAll(/([A-Z])/g, " $1")}
            </p>
            <p className="mt-1 break-words font-mono text-xs">
              {typeof value === "string" || typeof value === "number"
                ? String(value)
                : JSON.stringify(value)}
            </p>
          </div>
        ))}
      </section>

      {change.approvals.length ? (
        <section className="mt-4 space-y-2">
          {change.approvals.map((approval) => (
            <div
              className="rounded-xl border px-4 py-3"
              key={approval.approverId}
            >
              <div className="flex items-center justify-between gap-3">
                <p className="text-sm font-semibold">
                  {approval.approverDisplayName}
                </p>
                <Badge
                  variant={
                    approval.decision === "approve" ? "success" : "destructive"
                  }
                >
                  {approval.decision}
                </Badge>
              </div>
              {approval.comment ? (
                <p className="mt-1 text-xs text-muted-foreground">
                  {approval.comment}
                </p>
              ) : null}
            </div>
          ))}
        </section>
      ) : null}

      {change.status === "pending" ? (
        <section className="mt-5 rounded-2xl border bg-muted/20 p-5">
          <label
            className="text-xs font-medium text-muted-foreground"
            htmlFor="iam-review-comment"
          >
            Review comment
          </label>
          <Textarea
            className="mt-2"
            id="iam-review-comment"
            maxLength={500}
            onChange={(event) => onCommentChange(event.target.value)}
            placeholder="Record what you checked and why this decision is safe."
            value={comment}
          />
          <div className="mt-4 flex justify-end gap-2">
            <Button
              disabled={busy}
              onClick={() => void onDecide("reject")}
              variant="outline"
            >
              <ShieldX className="mr-2 size-4" />
              Reject change
            </Button>
            <Button disabled={busy} onClick={() => void onDecide("approve")}>
              <Check className="mr-2 size-4" />
              Approve review
            </Button>
          </div>
        </section>
      ) : null}
    </div>
  );
}

function EmptyState({ detail, title }: { detail: string; title: string }) {
  return (
    <div className="grid min-h-56 flex-1 place-items-center p-8 text-center">
      <div>
        <FileClock className="mx-auto size-6 text-muted-foreground" />
        <h3 className="mt-4 text-base font-semibold">{title}</h3>
        <p className="mt-1 max-w-sm text-sm text-muted-foreground">{detail}</p>
      </div>
    </div>
  );
}
