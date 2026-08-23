import * as React from "react";
import { Bot, KeyRound, Search, Shield, UserRound } from "lucide-react";

import type {
  IamCatalog,
  IamPrincipal,
} from "@/features/business-iam-admin/businessIamAdminApi";
import { Badge } from "@/shared/ui/badge";
import { Input } from "@/shared/ui/input";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/shared/ui/tabs";

export function AuthorityCatalog({ catalog }: { catalog: IamCatalog }) {
  const [query, setQuery] = React.useState("");
  const normalized = query.trim().toLowerCase();
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="relative mb-3">
        <Search className="absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
        <Input
          aria-label="Search authority catalog"
          className="pl-9"
          onChange={(event) => setQuery(event.target.value)}
          placeholder="Search people, agents, roles, or capabilities"
          value={query}
        />
      </div>
      <Tabs className="flex min-h-0 flex-1 flex-col" defaultValue="principals">
        <TabsList className="w-fit">
          <TabsTrigger value="principals">Principals</TabsTrigger>
          <TabsTrigger value="roles">Roles</TabsTrigger>
          <TabsTrigger value="permissions">Capabilities</TabsTrigger>
        </TabsList>
        <TabsContent
          className="min-h-0 flex-1 overflow-auto"
          value="principals"
        >
          <div className="grid gap-3 md:grid-cols-2">
            {catalog.principals
              .filter((item) =>
                `${item.displayName} ${item.externalId} ${item.kind}`
                  .toLowerCase()
                  .includes(normalized),
              )
              .map((principal) => (
                <PrincipalCard key={principal.id} principal={principal} />
              ))}
          </div>
        </TabsContent>
        <TabsContent className="min-h-0 flex-1 overflow-auto" value="roles">
          <div className="grid gap-3 md:grid-cols-2">
            {catalog.roles
              .filter((item) =>
                `${item.code} ${item.name}`.toLowerCase().includes(normalized),
              )
              .map((role) => (
                <article
                  className="rounded-xl border bg-card p-4"
                  key={role.id}
                >
                  <div className="flex items-start justify-between gap-3">
                    <div>
                      <p className="text-sm font-semibold">{role.name}</p>
                      <p className="mt-1 font-mono text-xs text-muted-foreground">
                        {role.code}
                      </p>
                    </div>
                    <Badge
                      variant={
                        role.status === "active" ? "success" : "secondary"
                      }
                    >
                      {role.status}
                    </Badge>
                  </div>
                  <div className="mt-4 space-y-1.5">
                    {role.permissions.map((permission) => (
                      <p
                        className="flex items-center gap-2 text-xs"
                        key={permission.capability}
                      >
                        <KeyRound className="size-3 text-muted-foreground" />
                        <span className="font-mono">
                          {permission.capability}
                        </span>
                      </p>
                    ))}
                    {role.permissions.length === 0 ? (
                      <p className="text-xs text-muted-foreground">
                        No capabilities assigned.
                      </p>
                    ) : null}
                  </div>
                </article>
              ))}
          </div>
        </TabsContent>
        <TabsContent
          className="min-h-0 flex-1 overflow-auto"
          value="permissions"
        >
          <div className="overflow-hidden rounded-xl border">
            {catalog.permissions
              .filter((item) =>
                `${item.capability} ${item.resourceType} ${item.action}`
                  .toLowerCase()
                  .includes(normalized),
              )
              .map((permission) => (
                <div
                  className="flex items-center justify-between gap-4 border-b px-4 py-3 last:border-0"
                  key={permission.id}
                >
                  <div className="min-w-0">
                    <p className="truncate font-mono text-sm">
                      {permission.capability}
                    </p>
                    <p className="mt-1 text-xs text-muted-foreground">
                      {permission.obligations.length
                        ? permission.obligations.join(" · ")
                        : "No additional obligation"}
                    </p>
                  </div>
                  <Badge
                    variant={
                      permission.riskLevel === "critical"
                        ? "destructive"
                        : permission.riskLevel === "high"
                          ? "warning"
                          : "secondary"
                    }
                  >
                    {permission.riskLevel}
                  </Badge>
                </div>
              ))}
          </div>
        </TabsContent>
      </Tabs>
    </div>
  );
}

function PrincipalCard({ principal }: { principal: IamPrincipal }) {
  const Icon = principal.kind === "human" ? UserRound : Bot;
  return (
    <article className="rounded-xl border bg-card p-4">
      <div className="flex items-start justify-between gap-3">
        <div className="flex min-w-0 gap-3">
          <div className="grid size-9 shrink-0 place-items-center rounded-lg bg-muted">
            <Icon className="size-4" />
          </div>
          <div className="min-w-0">
            <p className="truncate text-sm font-semibold">
              {principal.displayName}
            </p>
            <p className="mt-1 truncate font-mono text-xs text-muted-foreground">
              {principal.externalId}
            </p>
          </div>
        </div>
        <Badge
          variant={principal.status === "active" ? "success" : "secondary"}
        >
          {principal.status}
        </Badge>
      </div>
      <div className="mt-4 flex flex-wrap gap-1.5">
        <Badge variant="outline">
          <Shield className="mr-1 size-3" />
          {principal.kind.replaceAll("_", " ")}
        </Badge>
        {principal.roles.map((role) => (
          <Badge key={role.code} variant="secondary">
            {role.name}
          </Badge>
        ))}
      </div>
      <p className="mt-3 text-2xs text-muted-foreground">
        {principal.permissions.length} direct capabilities · version{" "}
        {principal.version}
      </p>
    </article>
  );
}
