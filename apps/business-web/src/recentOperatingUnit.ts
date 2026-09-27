import type { OperatingUnitRecord } from "./OperatingUnitTree";
import { request } from "./api.ts";

type OperatingUnitStorage = Pick<Storage, "getItem" | "setItem" | "removeItem">;

const STORAGE_PREFIX = "business.recent-operating-unit.";

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
    const response = await request<{ businessUnitId: string | null }>(
      `/api/v1/preferences/operating-unit/${encodeURIComponent(context)}`,
    );
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
    await request(
      `/api/v1/preferences/operating-unit/${encodeURIComponent(context)}`,
      {
        method: "PUT",
        body: JSON.stringify({ businessUnitId: id }),
      },
    );
  } catch {
    // The local value remains available until a later successful server sync.
  }
}

function browserStorage(): Storage | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}
