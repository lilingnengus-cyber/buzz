import { useEffect, useRef, useState } from "react";
import { request } from "./api";
import type { MasterPage } from "./masterDataPages";

export function useMasterDeepLink<T extends { id: string }>(endpoint: string, type: string, id: string | undefined, onFound: (record: T) => void) {
  const found = useRef(onFound);
  found.current = onFound;
  const [error, setError] = useState("");
  const [revision, setRevision] = useState(0);
  useEffect(() => {
    setError("");
    if (!id) return;
    const controller = new AbortController();
    const query = new URLSearchParams({ resourceType: type, id, limit: "1" });
    request<MasterPage<T>>(`${endpoint}?${query}`, { signal: controller.signal }).then((result) => {
      if (controller.signal.aborted) return;
      const record = result.items.find((item) => item.id === id);
      if (record) found.current(record);
      else setError("该记录不存在或无访问权限");
    }).catch((reason) => {
      if (!controller.signal.aborted) setError(reason instanceof Error ? reason.message : "详情加载失败，请重试");
    });
    return () => controller.abort();
  }, [endpoint, type, id, revision]);
  return { error, retry: () => setRevision((n) => n + 1) };
}
