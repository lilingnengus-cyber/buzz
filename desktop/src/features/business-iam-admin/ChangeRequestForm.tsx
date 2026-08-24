import * as React from "react";
import { ArrowRight, LockKeyhole } from "lucide-react";

import type {
  IamCatalog,
  IamOperation,
} from "@/features/business-iam-admin/businessIamAdminApi";
import {
  buildChangeRequest,
  type ChangeDraft,
  EMPTY_CHANGE_DRAFT,
} from "@/features/business-iam-admin/changeRequestDraft";
import { Button } from "@/shared/ui/button";
import { Checkbox } from "@/shared/ui/checkbox";
import { Input } from "@/shared/ui/input";
import { Textarea } from "@/shared/ui/textarea";

const OPERATIONS: Array<{ value: IamOperation; label: string }> = [
  { value: "permission_grant", label: "Grant direct permission" },
  { value: "permission_revoke", label: "Revoke direct permission" },
  { value: "role_assign", label: "Assign role" },
  { value: "role_unassign", label: "Remove role" },
  { value: "role_permission_grant", label: "Add permission to role" },
  { value: "role_permission_revoke", label: "Remove permission from role" },
  { value: "principal_upsert", label: "Add or restore principal" },
  { value: "principal_disable", label: "Disable principal" },
  { value: "role_upsert", label: "Add or restore role" },
  { value: "role_disable", label: "Disable role" },
];

const SELECT_CLASS =
  "h-9 w-full rounded-lg border border-input/40 bg-background px-3 text-sm focus-visible:outline-hidden focus-visible:ring-1 focus-visible:ring-ring";

export function ChangeRequestForm({
  catalog,
  busy,
  onCreate,
}: {
  catalog: IamCatalog;
  busy: boolean;
  onCreate: (input: ReturnType<typeof buildChangeRequest>) => Promise<void>;
}) {
  const [draft, setDraft] = React.useState<ChangeDraft>(EMPTY_CHANGE_DRAFT);
  const [error, setError] = React.useState<string | null>(null);
  const operation = draft.operation;
  const principalTarget = [
    "principal_disable",
    "permission_grant",
    "permission_revoke",
    "role_assign",
    "role_unassign",
  ].includes(operation);
  const roleTarget = [
    "role_disable",
    "role_permission_grant",
    "role_permission_revoke",
    "role_assign",
    "role_unassign",
  ].includes(operation);
  const capabilityTarget = [
    "permission_grant",
    "permission_revoke",
    "role_permission_grant",
    "role_permission_revoke",
  ].includes(operation);
  const grant = ["permission_grant", "role_permission_grant"].includes(
    operation,
  );
  const update = <K extends keyof ChangeDraft>(key: K, value: ChangeDraft[K]) =>
    setDraft((current) => ({ ...current, [key]: value }));

  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    setError(null);
    try {
      await onCreate(buildChangeRequest(draft, catalog));
      setDraft((current) => ({
        ...EMPTY_CHANGE_DRAFT,
        operation: current.operation,
      }));
    } catch (cause) {
      setError(
        cause instanceof Error
          ? cause.message
          : "The request could not be created.",
      );
    }
  };

  return (
    <form className="min-h-0 flex-1 overflow-auto pr-1" onSubmit={submit}>
      <div className="mx-auto max-w-2xl space-y-5">
        <section className="rounded-xl border bg-card p-5">
          <div className="flex items-center gap-3">
            <div className="grid size-9 place-items-center rounded-lg bg-primary/10 text-primary">
              <LockKeyhole className="size-4" />
            </div>
            <div>
              <h3 className="text-sm font-semibold">
                Describe one authority change
              </h3>
              <p className="mt-0.5 text-xs text-muted-foreground">
                The request is version-bound and does not change authority until
                reviewed.
              </p>
            </div>
          </div>

          <Field label="Change">
            <select
              className={SELECT_CLASS}
              onChange={(event) =>
                setDraft({
                  ...EMPTY_CHANGE_DRAFT,
                  operation: event.target.value as IamOperation,
                })
              }
              value={operation}
            >
              {OPERATIONS.map((item) => (
                <option key={item.value} value={item.value}>
                  {item.label}
                </option>
              ))}
            </select>
          </Field>

          {operation === "principal_upsert" ? (
            <div className="grid gap-4 sm:grid-cols-2">
              <Field label="Principal type">
                <select
                  className={SELECT_CLASS}
                  onChange={(event) =>
                    update(
                      "principalKind",
                      event.target.value as ChangeDraft["principalKind"],
                    )
                  }
                  value={draft.principalKind}
                >
                  <option value="human">Human</option>
                  <option value="independent_agent">Independent Agent</option>
                </select>
              </Field>
              <Field label="External ID">
                <Input
                  onChange={(event) => update("externalId", event.target.value)}
                  placeholder="finance-agent"
                  value={draft.externalId}
                />
              </Field>
              <Field className="sm:col-span-2" label="Display name">
                <Input
                  onChange={(event) =>
                    update("displayName", event.target.value)
                  }
                  placeholder="Finance digital employee"
                  value={draft.displayName}
                />
              </Field>
            </div>
          ) : null}

          {operation === "role_upsert" ? (
            <div className="grid gap-4 sm:grid-cols-2">
              <Field label="Role code">
                <Input
                  onChange={(event) => update("roleCode", event.target.value)}
                  placeholder="finance.operator"
                  value={draft.roleCode}
                />
              </Field>
              <Field label="Role name">
                <Input
                  onChange={(event) => update("roleName", event.target.value)}
                  placeholder="Finance operator"
                  value={draft.roleName}
                />
              </Field>
            </div>
          ) : null}

          {principalTarget ? (
            <Field label="Principal">
              <select
                className={SELECT_CLASS}
                onChange={(event) => update("principalId", event.target.value)}
                value={draft.principalId}
              >
                <option value="">Choose a person or Agent</option>
                {catalog.principals
                  .filter((item) => item.status === "active")
                  .map((item) => (
                    <option key={item.id} value={item.id}>
                      {item.displayName} · {item.kind.replaceAll("_", " ")} · v
                      {item.version}
                    </option>
                  ))}
              </select>
            </Field>
          ) : null}

          {roleTarget ? (
            <Field label="Role">
              <select
                className={SELECT_CLASS}
                onChange={(event) => update("roleId", event.target.value)}
                value={draft.roleId}
              >
                <option value="">Choose a role</option>
                {catalog.roles
                  .filter((item) => item.status === "active")
                  .map((item) => (
                    <option key={item.id} value={item.id}>
                      {item.name} · {item.code} · v{item.version}
                    </option>
                  ))}
              </select>
            </Field>
          ) : null}

          {capabilityTarget ? (
            <Field label="Capability">
              <select
                className={SELECT_CLASS}
                onChange={(event) => update("capability", event.target.value)}
                value={draft.capability}
              >
                <option value="">Choose a capability</option>
                {catalog.permissions
                  .filter((item) => item.status === "active")
                  .map((item) => (
                    <option key={item.id} value={item.capability}>
                      {item.capability} · {item.riskLevel}
                    </option>
                  ))}
              </select>
            </Field>
          ) : null}

          {grant ? (
            <div className="rounded-lg border bg-muted/30 p-4">
              <p className="text-xs font-semibold">Data boundary</p>
              <div className="mt-3 grid gap-4 sm:grid-cols-2">
                <Field label="Scope">
                  <select
                    className={SELECT_CLASS}
                    onChange={(event) =>
                      update(
                        "scopeMode",
                        event.target.value as ChangeDraft["scopeMode"],
                      )
                    }
                    value={draft.scopeMode}
                  >
                    <option value="unrestricted">Capability default</option>
                    <option value="restricted">Restrict by dimension</option>
                  </select>
                </Field>
                {draft.scopeMode === "restricted" ? (
                  <>
                    <Field label="Dimension">
                      <Input
                        onChange={(event) =>
                          update("scopeDimension", event.target.value)
                        }
                        placeholder="warehouse"
                        value={draft.scopeDimension}
                      />
                    </Field>
                    <Field className="sm:col-span-2" label="Allowed values">
                      <Input
                        onChange={(event) =>
                          update("scopeValues", event.target.value)
                        }
                        placeholder="sh-01, sh-02"
                        value={draft.scopeValues}
                      />
                    </Field>
                  </>
                ) : null}
              </div>
              <div className="mt-4 flex flex-wrap gap-4">
                {[
                  ["human_approval", "Human approval"],
                  ["step_up_authentication", "Step-up"],
                  ["dual_control", "Dual control"],
                ].map(([value, label]) => (
                  <label
                    className="flex items-center gap-2 text-xs"
                    htmlFor={`iam-obligation-${value}`}
                    key={value}
                  >
                    <Checkbox
                      checked={draft.obligations.includes(value)}
                      id={`iam-obligation-${value}`}
                      onCheckedChange={(checked) =>
                        update(
                          "obligations",
                          checked
                            ? [...draft.obligations, value]
                            : draft.obligations.filter(
                                (item) => item !== value,
                              ),
                        )
                      }
                    />
                    {label}
                  </label>
                ))}
              </div>
            </div>
          ) : null}

          <Field label="Business reason">
            <Textarea
              maxLength={500}
              onChange={(event) => update("reason", event.target.value)}
              placeholder="State the business need, data boundary, and expected duration."
              value={draft.reason}
            />
          </Field>
        </section>
        {error ? (
          <p className="rounded-lg bg-destructive/10 px-3 py-2 text-sm text-destructive">
            {error}
          </p>
        ) : null}
        <div className="flex items-center justify-between gap-4">
          <p className="text-xs text-muted-foreground">
            Sensitive changes automatically require two independent reviewers.
          </p>
          <Button disabled={busy} type="submit">
            {busy ? "Creating…" : "Create review request"}
            <ArrowRight className="ml-2 size-4" />
          </Button>
        </div>
      </div>
    </form>
  );
}

function Field({
  children,
  className,
  label,
}: {
  children: React.ReactNode;
  className?: string;
  label: string;
}) {
  const generatedId = React.useId();
  const control = React.Children.only(children) as React.ReactElement<{
    id?: string;
  }>;
  const controlId = control.props.id ?? generatedId;

  return (
    <div className={`mt-4 space-y-1.5 ${className ?? ""}`}>
      <label
        className="block text-xs font-medium text-muted-foreground"
        htmlFor={controlId}
      >
        {label}
      </label>
      {React.cloneElement(control, { id: controlId })}
    </div>
  );
}
