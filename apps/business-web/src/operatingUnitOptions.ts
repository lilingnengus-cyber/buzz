import {
  type CoreMasterList,
  type CoreMasterRecord,
  request,
} from "./api";

export async function loadOperatingUnits(): Promise<CoreMasterRecord[]> {
  const result = await request<CoreMasterList>(
    "/api/v1/core-master-data?limit=1000",
  );
  return result.items.filter(
    (item) => item.resourceType === "business_unit" && item.status === "active",
  );
}
