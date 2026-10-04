import React from "react";
import { loadAllMasterData } from "./masterDataPages";

const PRODUCT_OPTIONS: Record<string, string[]> = {
  product: ["product_category", "brand", "unit_of_measure"],
  sku: ["product"],
  product_category: ["product_category"],
  uom_conversion: ["product", "unit_of_measure"],
};

export function MasterFormOptions<T extends { id: string; resourceType: string }>({ endpoint, type, children, onClose }: {
  endpoint: string;
  type: string;
  children: (items: T[]) => React.ReactNode;
  onClose: () => void;
}) {
  const [data, setData] = React.useState<T[] | null>(null);
  const [error, setError] = React.useState("");
  const [revision, setRevision] = React.useState(0);
  React.useEffect(() => {
    const controller = new AbortController();
    setData(null);
    setError("");
    const types = endpoint.includes("product-master") ? PRODUCT_OPTIONS[type] ?? [] : type === "business_unit" ? ["business_unit"] : [];
    Promise.all(types.map((kind) => loadAllMasterData<T>(endpoint, kind, controller.signal))).then((pages) => {
      if (!controller.signal.aborted) setData([...new Map(pages.flatMap((page) => page.items).map((item) => [`${item.resourceType}:${item.id}`, item])).values()]);
    }).catch((reason) => {
      if (!controller.signal.aborted) setError(reason instanceof Error ? reason.message : "候选资料加载失败");
    });
    return () => controller.abort();
  }, [endpoint, type, revision]);
  if (data) return children(data);
  return <div role={error ? "alert" : "status"} className="master-message">
    <p>{error || "正在加载可选资料…"}</p>
    {error && <button type="button" onClick={() => setRevision((n) => n + 1)}>重试</button>}
    <button type="button" onClick={onClose}>取消</button>
  </div>;
}
