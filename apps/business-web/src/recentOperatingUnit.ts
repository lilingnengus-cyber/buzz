import type { OperatingUnitRecord } from "./OperatingUnitTree";

type OperatingUnitStorage = Pick<Storage, "getItem" | "setItem">;

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

function browserStorage(): Storage | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}
