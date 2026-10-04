const paths: Record<string, string> = {
  customer: "customers", supplier: "suppliers", warehouse: "warehouses",
  product: "products", sku: "skus",
};

export function MasterRecordLink({ type, id }: { type: string; id?: string }) {
  if (!id || !paths[type]) return null;
  const prefix = window.location.pathname.startsWith("/embed/") ? "/embed" : "";
  return <a href={`${prefix}/${paths[type]}/${encodeURIComponent(id)}`}>详情链接</a>;
}
