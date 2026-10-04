import React from "react";
import { request, toApiFailure, type ApiFailure } from "./api";
import { loadAllMasterData, type MasterPage } from "./masterDataPages";

const PAGE_SIZE = 50;
export function useMasterPage<T>(endpoint: string, resourceType: string, query: string, status: string, tree = false) {
  const key = JSON.stringify([endpoint, resourceType, query, status]);
  const [position, setPosition] = React.useState({ key, offset: 0 });
  const offset = position.key === key ? position.offset : 0;
  const [revision, setRevision] = React.useState(0);
  const [data, setData] = React.useState<MasterPage<T> | null>(null);
  const [loading, setLoading] = React.useState(true);
  const [error, setError] = React.useState<ApiFailure | null>(null);
  React.useEffect(() => {
    const controller = new AbortController();
    setLoading(true);
    setError(null);
    const params = new URLSearchParams({ resourceType, query: query.trim(), offset: String(offset), limit: String(PAGE_SIZE) });
    if (status !== "all") params.set("status", status);
    const pending = tree
      ? loadAllMasterData<T>(endpoint, resourceType, controller.signal)
      : request<MasterPage<T>>(`${endpoint}?${params}`, { signal: controller.signal });
    pending.then((next) => {
      if (controller.signal.aborted) return;
      if (!tree && next.total !== undefined && offset > 0 && offset >= next.total) {
        setPosition({ key, offset: Math.max(0, Math.floor((next.total - 1) / PAGE_SIZE) * PAGE_SIZE) });
      }
      setData(next);
    }).catch((reason) => {
      if (!controller.signal.aborted) setError(toApiFailure(reason, "基础资料加载失败"));
    }).finally(() => {
      if (!controller.signal.aborted) setLoading(false);
    });
    return () => controller.abort();
  }, [endpoint, resourceType, query, status, offset, revision, tree, key]);
  const load = React.useCallback(async () => { setRevision((n) => n + 1); }, []);
  const pagination = !tree && data && !error && (data.total !== undefined || data.hasMore) ? (
    <nav className="master-toolbar" aria-label="资料分页">
      <span>{data.total !== undefined ? `共 ${data.total} 条，` : ""}第 {Math.floor(offset / PAGE_SIZE) + 1} 页</span>
      <button type="button" disabled={loading || offset === 0} onClick={() => setPosition({ key, offset: Math.max(0, offset - PAGE_SIZE) })}>上一页</button>
      <button type="button" disabled={loading || !data.hasMore} onClick={() => setPosition({ key, offset: offset + PAGE_SIZE })}>下一页</button>
    </nav>
  ) : null;
  return { data, loading, error, load, pagination };
}
