import { useState } from "react";

type Filter = { query: string; status: string };
export function useMasterFilters<T extends string>(type: T) {
  const [filters, setFilters] = useState<Partial<Record<T, Filter>>>({});
  const { query = "", status = "all" } = filters[type] ?? {};
  const setForType = (target: T, filter: Filter) =>
    setFilters((current) => ({ ...current, [target]: filter }));
  return {
    query,
    status,
    setForType,
    setQuery: (query: string) => setForType(type, { query, status }),
    setStatus: (status: string) => setForType(type, { query, status }),
  };
}
