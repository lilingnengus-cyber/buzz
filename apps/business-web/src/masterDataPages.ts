import { request } from "./api";

export type MasterPage<T> = {
  items: T[];
  canManage: boolean;
  dataAsOf: string;
  total?: number;
  hasMore?: boolean;
  counts?: Record<string, number>;
};

// Trees and form candidates need complete scoped results, not a single list page.
export async function loadAllMasterData<T>(endpoint: string, resourceType: string, signal?: AbortSignal): Promise<MasterPage<T>> {
  const items: T[] = [];
  let offset = 0;
  for (;;) {
    const params = new URLSearchParams({ resourceType, offset: String(offset), limit: "500" });
    const page = await request<MasterPage<T>>(`${endpoint}?${params}`, { signal });
    items.push(...page.items);
    if (!page.hasMore) return { ...page, items, hasMore: false };
    if (!page.items.length) throw new Error("资料分页返回空页，请刷新重试");
    offset += page.items.length;
  }
}
