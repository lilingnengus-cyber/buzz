import type { OperatingUnitRecord } from "./OperatingUnitTree";
import { request } from "./api.ts";

type OperatingUnitStorage = Pick<Storage, "getItem" | "setItem" | "removeItem">;

const STORAGE_PREFIX = "business.recent-operating-unit.";

export type AccountPreference = {
  context: string;
  businessUnitId: string | null;
  pinned: boolean;
};

export function resolveRecentOperatingUnit(
  context: string,
  records: OperatingUnitRecord[],
  fallback = "",
  storage: OperatingUnitStorage | null = browserStorage(),
) {
  let recent: string | null = null;
  try {
    recent = storage?.getItem(`${STORAGE_PREFIX}${context}`) ?? null;
  } catch {
    return fallback;
  }
  return records.some(
    (record) => record.id === recent && record.status === "active",
  )
    ? (recent ?? fallback)
    : fallback;
}

export function rememberRecentOperatingUnit(
  context: string,
  id: string,
  storage: OperatingUnitStorage | null = browserStorage(),
) {
  if (!id) return;
  try {
    storage?.setItem(`${STORAGE_PREFIX}${context}`, id);
  } catch {
    // Browser privacy settings can disable storage; saving the business record
    // must still succeed when that happens.
  }
}

export async function resolveSyncedRecentOperatingUnit(
  context: string,
  records: OperatingUnitRecord[],
  fallback = "",
  storage: OperatingUnitStorage | null = browserStorage(),
) {
  try {
    const response = await loadAccountOperatingUnitPreference(context);
    const remote = response.businessUnitId;
    if (
      remote &&
      records.some(
        (record) => record.id === remote && record.status === "active",
      )
    ) {
      rememberRecentOperatingUnit(context, remote, storage);
      return remote;
    }
    try {
      storage?.removeItem(`${STORAGE_PREFIX}${context}`);
    } catch {
      // The server result is still authoritative when local storage is blocked.
    }
    return fallback;
  } catch {
    return resolveRecentOperatingUnit(context, records, fallback, storage);
  }
}

export async function rememberSyncedRecentOperatingUnit(
  context: string,
  id: string,
  storage: OperatingUnitStorage | null = browserStorage(),
) {
  rememberRecentOperatingUnit(context, id, storage);
  try {
    await saveAccountOperatingUnitPreference(context, id);
  } catch {
    // The local value remains available until a later successful server sync.
  }
}

export function loadAccountOperatingUnitPreference(context: string) {
  return request<AccountPreference>(
    `/api/v1/preferences/operating-unit/${encodeURIComponent(context)}`,
  );
}

export function saveAccountOperatingUnitPreference(
  context: string,
  businessUnitId: string,
  pinned = false,
) {
  return request<AccountPreference>(
    `/api/v1/preferences/operating-unit/${encodeURIComponent(context)}`,
    {
      method: "PUT",
      body: JSON.stringify({ businessUnitId, pinned }),
    },
  );
}

export async function clearAccountOperatingUnitPreference(
  context: string,
  storage: OperatingUnitStorage | null = browserStorage(),
) {
  const response = await request<AccountPreference>(
    `/api/v1/preferences/operating-unit/${encodeURIComponent(context)}`,
    { method: "DELETE" },
  );
  try {
    storage?.removeItem(`${STORAGE_PREFIX}${context}`);
  } catch {
    // The account preference is already cleared even if local storage is blocked.
  }
  return response;
}

function browserStorage(): Storage | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}
