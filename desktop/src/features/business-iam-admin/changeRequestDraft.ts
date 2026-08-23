import type {
  IamCatalog,
  IamOperation,
} from "@/features/business-iam-admin/businessIamAdminApi";

export type ChangeDraft = {
  operation: IamOperation;
  principalId: string;
  roleId: string;
  capability: string;
  principalKind: "human" | "independent_agent" | "proxy_agent";
  externalId: string;
  displayName: string;
  roleCode: string;
  roleName: string;
  scopeMode: "unrestricted" | "restricted";
  scopeDimension: string;
  scopeValues: string;
  obligations: string[];
  reason: string;
};

export const EMPTY_CHANGE_DRAFT: ChangeDraft = {
  operation: "permission_grant",
  principalId: "",
  roleId: "",
  capability: "",
  principalKind: "human",
  externalId: "",
  displayName: "",
  roleCode: "",
  roleName: "",
  scopeMode: "unrestricted",
  scopeDimension: "legal_entity",
  scopeValues: "",
  obligations: [],
  reason: "",
};

export function buildChangeRequest(
  draft: ChangeDraft,
  catalog: IamCatalog,
): {
  operation: IamOperation;
  payload: Record<string, unknown>;
  reason: string;
} {
  const reason = required(
    draft.reason,
    "Explain why this authority change is needed.",
  );
  let payload: Record<string, unknown>;
  switch (draft.operation) {
    case "principal_upsert": {
      const externalId = required(draft.externalId, "Enter an external ID.");
      const existing = catalog.principals.find(
        (item) =>
          item.kind === draft.principalKind && item.externalId === externalId,
      );
      payload = {
        kind: draft.principalKind,
        externalId,
        displayName: required(draft.displayName, "Enter a display name."),
        ...(existing ? { expectedVersion: existing.version } : {}),
      };
      break;
    }
    case "principal_disable": {
      const principal = principalFromDraft(draft, catalog);
      payload = {
        externalId: principal.externalId,
        expectedVersion: principal.version,
      };
      break;
    }
    case "role_upsert": {
      const code = required(draft.roleCode, "Enter a role code.");
      const existing = catalog.roles.find((item) => item.code === code);
      payload = {
        code,
        name: required(draft.roleName, "Enter a role name."),
        ...(existing ? { expectedVersion: existing.version } : {}),
      };
      break;
    }
    case "role_disable": {
      const role = roleFromDraft(draft, catalog);
      payload = { code: role.code, expectedVersion: role.version };
      break;
    }
    case "permission_grant": {
      const principal = principalFromDraft(draft, catalog);
      payload = {
        externalId: principal.externalId,
        capability: capabilityFromDraft(draft, catalog),
        dataScope: dataScopeFromDraft(draft),
        obligations: draft.obligations,
        expectedVersion: principal.version,
      };
      break;
    }
    case "permission_revoke": {
      const principal = principalFromDraft(draft, catalog);
      payload = {
        externalId: principal.externalId,
        capability: capabilityFromDraft(draft, catalog),
        expectedVersion: principal.version,
      };
      break;
    }
    case "role_permission_grant": {
      const role = roleFromDraft(draft, catalog);
      payload = {
        role: role.code,
        capability: capabilityFromDraft(draft, catalog),
        dataScope: dataScopeFromDraft(draft),
        obligations: draft.obligations,
        expectedVersion: role.version,
      };
      break;
    }
    case "role_permission_revoke": {
      const role = roleFromDraft(draft, catalog);
      payload = {
        role: role.code,
        capability: capabilityFromDraft(draft, catalog),
        expectedVersion: role.version,
      };
      break;
    }
    case "role_assign":
    case "role_unassign": {
      const principal = principalFromDraft(draft, catalog);
      const role = roleFromDraft(draft, catalog);
      payload = {
        externalId: principal.externalId,
        role: role.code,
        expectedVersion: principal.version,
      };
      break;
    }
  }
  return { operation: draft.operation, payload, reason };
}

function required(value: string, message: string) {
  const normalized = value.trim();
  if (!normalized) throw new Error(message);
  return normalized;
}

function principalFromDraft(draft: ChangeDraft, catalog: IamCatalog) {
  const principal = catalog.principals.find(
    (item) => item.id === draft.principalId && item.status === "active",
  );
  if (!principal) throw new Error("Choose an active principal.");
  return principal;
}

function roleFromDraft(draft: ChangeDraft, catalog: IamCatalog) {
  const role = catalog.roles.find(
    (item) => item.id === draft.roleId && item.status === "active",
  );
  if (!role) throw new Error("Choose an active role.");
  return role;
}

function capabilityFromDraft(draft: ChangeDraft, catalog: IamCatalog) {
  const permission = catalog.permissions.find(
    (item) => item.capability === draft.capability && item.status === "active",
  );
  if (!permission) throw new Error("Choose an active capability.");
  return permission.capability;
}

function dataScopeFromDraft(draft: ChangeDraft) {
  if (draft.scopeMode === "unrestricted") return { mode: "unrestricted" };
  const dimension = required(
    draft.scopeDimension,
    "Enter a data-scope dimension.",
  );
  const values = [
    ...new Set(
      draft.scopeValues
        .split(",")
        .map((value) => value.trim())
        .filter(Boolean),
    ),
  ];
  if (values.length === 0)
    throw new Error("Enter at least one allowed scope value.");
  return { mode: "restricted", dimensions: { [dimension]: values } };
}
