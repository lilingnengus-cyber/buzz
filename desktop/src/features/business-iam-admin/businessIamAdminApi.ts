export type IamOperation =
  | "principal_upsert"
  | "principal_disable"
  | "role_upsert"
  | "role_disable"
  | "permission_grant"
  | "permission_revoke"
  | "role_permission_grant"
  | "role_permission_revoke"
  | "role_assign"
  | "role_unassign";

export type IamPermissionAssignment = {
  capability: string;
  dataScope: unknown;
  obligations: string[];
};

export type IamPrincipal = {
  id: string;
  kind: "human" | "independent_agent";
  externalId: string;
  displayName: string;
  status: "active" | "disabled";
  version: number;
  updatedAt: string;
  roles: Array<{ code: string; name: string }>;
  permissions: IamPermissionAssignment[];
};

export type IamRole = {
  id: string;
  code: string;
  name: string;
  status: "active" | "disabled";
  version: number;
  updatedAt: string;
  permissions: IamPermissionAssignment[];
};

export type IamPermission = {
  id: string;
  capability: string;
  resourceType: string;
  action: string;
  riskLevel: "low" | "medium" | "high" | "critical";
  status: "active" | "disabled";
  obligations: string[];
  defaultDataScope: unknown;
  version: number;
};

export type IamCatalog = {
  principals: IamPrincipal[];
  roles: IamRole[];
  permissions: IamPermission[];
};

export type IamApproval = {
  approverId: string;
  approverDisplayName: string;
  decision: "approve" | "reject";
  comment?: string | null;
  decidedAt: string;
};

export type IamChangeRequest = {
  id: string;
  operation: IamOperation;
  payload: Record<string, unknown>;
  riskLevel: "high" | "critical";
  requiredApprovals: number;
  approvalCount: number;
  status:
    | "pending"
    | "approved"
    | "rejected"
    | "applied"
    | "failed"
    | "cancelled";
  requestedBy: string;
  requesterDisplayName: string;
  approvals: IamApproval[];
  reason: string;
  traceId: string;
  requestedAt: string;
  expiresAt: string;
  decidedAt?: string | null;
  appliedAt?: string | null;
  failureCode?: string | null;
  version: number;
};

export class BusinessIamApiError extends Error {
  readonly status: number;
  readonly code: string;

  constructor(status: number, code: string) {
    super(code);
    this.name = "BusinessIamApiError";
    this.status = status;
    this.code = code;
  }
}

async function api<T>(
  baseUrl: string,
  accessToken: string,
  path: string,
  init?: RequestInit,
): Promise<T> {
  const response = await fetch(new URL(path, `${baseUrl}/`), {
    ...init,
    cache: "no-store",
    credentials: "omit",
    headers: {
      Accept: "application/json",
      Authorization: `Bearer ${accessToken}`,
      ...(init?.body ? { "Content-Type": "application/json" } : {}),
      ...init?.headers,
    },
  });
  if (!response.ok) {
    const body = (await response.json().catch(() => null)) as {
      error?: unknown;
    } | null;
    throw new BusinessIamApiError(
      response.status,
      typeof body?.error === "string" ? body.error : "request_failed",
    );
  }
  return (await response.json()) as T;
}

export function readIamCatalog(baseUrl: string, accessToken: string) {
  return api<IamCatalog>(baseUrl, accessToken, "/api/iam/catalog");
}

export function readIamChanges(baseUrl: string, accessToken: string) {
  return api<IamChangeRequest[]>(
    baseUrl,
    accessToken,
    "/api/iam/change-requests",
  );
}

export function createIamChange(
  baseUrl: string,
  accessToken: string,
  input: {
    operation: IamOperation;
    payload: Record<string, unknown>;
    reason: string;
    idempotencyKey: string;
  },
) {
  return api<IamChangeRequest>(
    baseUrl,
    accessToken,
    "/api/iam/change-requests",
    { method: "POST", body: JSON.stringify(input) },
  );
}

export function decideIamChange(
  baseUrl: string,
  accessToken: string,
  id: string,
  decision: "approve" | "reject",
  comment: string,
) {
  return api<IamChangeRequest>(
    baseUrl,
    accessToken,
    `/api/iam/change-requests/${encodeURIComponent(id)}/${decision}`,
    { method: "POST", body: JSON.stringify({ comment }) },
  );
}

export function describeIamError(error: unknown): string {
  if (!(error instanceof BusinessIamApiError))
    return error instanceof Error
      ? error.message
      : "Business IAM is unavailable.";
  const messages: Record<string, string> = {
    business_iam_permission_denied:
      "Your Business IAM role does not allow this action.",
    approver_already_decided: "You already reviewed this change.",
    change_request_not_pending: "This change is no longer pending.",
    change_request_expired: "This change expired. Create a new request.",
    idempotency_key_reused:
      "This request key was already used for different content.",
    database_unavailable: "Business IAM storage is unavailable.",
  };
  return (
    messages[error.code] ?? `Business IAM rejected the request (${error.code}).`
  );
}
