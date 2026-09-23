export type OperatingUnitRecord = {
  id: string;
  code: string;
  name: string;
  status: string;
  resourceType: string;
  parentBusinessUnitId: string | null;
  ancestorPath: string[] | null;
  depth: number | null;
  descendantCount: number | null;
};

export type OperatingUnitNode = OperatingUnitRecord & {
  ancestorPath: string[];
  depth: number;
  descendantCount: number;
  children: OperatingUnitNode[];
  orphaned?: boolean;
};

const compare = (left: OperatingUnitNode, right: OperatingUnitNode) =>
  left.code.localeCompare(right.code, "zh-CN");

export function buildOperatingTree(
  records: OperatingUnitRecord[],
  query = "",
): OperatingUnitNode[] {
  const nodes = new Map<string, OperatingUnitNode>(
    records.map((record) => [
      record.id,
      {
        ...record,
        ancestorPath: record.ancestorPath ?? [],
        depth: record.depth ?? 0,
        descendantCount: record.descendantCount ?? 0,
        children: [],
      },
    ]),
  );
  const roots: OperatingUnitNode[] = [];
  const orphans: OperatingUnitNode[] = [];
  for (const node of nodes.values()) {
    if (!node.parentBusinessUnitId) roots.push(node);
    else {
      const parent = nodes.get(node.parentBusinessUnitId);
      if (parent) parent.children.push(node);
      else orphans.push({ ...node, orphaned: true });
    }
  }
  const sort = (node: OperatingUnitNode) => {
    node.children.sort(compare);
    node.children.forEach(sort);
  };
  roots.sort(compare);
  roots.forEach(sort);
  orphans.sort(compare);
  const needle = query.trim().toLocaleLowerCase();
  const retainMatches = (node: OperatingUnitNode): OperatingUnitNode | null => {
    const children = node.children
      .map(retainMatches)
      .filter((child): child is OperatingUnitNode => child !== null);
    const haystack =
      `${node.code} ${node.name} ${node.ancestorPath.join(" ")}`.toLocaleLowerCase();
    return !needle || haystack.includes(needle) || children.length > 0
      ? { ...node, children }
      : null;
  };
  const visible = roots
    .map(retainMatches)
    .filter((node): node is OperatingUnitNode => node !== null);
  const visibleOrphans = orphans
    .map(retainMatches)
    .filter((node): node is OperatingUnitNode => node !== null);
  if (visibleOrphans.length > 0) {
    visible.push({
      id: "__orphans__",
      code: "UNASSIGNED",
      name: "未归入树",
      status: "disabled",
      resourceType: "business_unit",
      parentBusinessUnitId: null,
      ancestorPath: [],
      depth: 0,
      descendantCount: visibleOrphans.length,
      children: visibleOrphans,
      orphaned: true,
    });
  }
  return visible;
}

export function updateIndependentSelection<
  T extends {
    legalEntityId: string;
    businessUnitId: string;
  },
>(state: T, field: "legalEntityId" | "businessUnitId", value: string): T {
  return { ...state, [field]: value };
}
