import { loadAllMasterData } from "./masterDataPages";
import { type CoreMasterRecord } from "./api";

export async function loadOperatingUnits(): Promise<CoreMasterRecord[]> {
  const result = await loadAllMasterData<CoreMasterRecord>("/api/v1/core-master-data", "business_unit");
  return result.items.filter(
    (item) => item.resourceType === "business_unit" && item.status === "active",
  );
}
