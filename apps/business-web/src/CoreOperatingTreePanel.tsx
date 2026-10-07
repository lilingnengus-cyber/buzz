import React from "react";
import type { CoreMasterRecord } from "./api";
import {
  buildOperatingTree,
  type OperatingUnitNode,
} from "./OperatingUnitTree";

export function OperatingTreePanel({
  records,
  query,
  canManage,
  onEdit,
  onStatus,
  onDelete,
}: {
  records: CoreMasterRecord[];
  query: string;
  canManage: boolean;
  onEdit: (record: CoreMasterRecord, detail?: boolean) => void;
  onStatus: (record: CoreMasterRecord) => void;
  onDelete: (record: CoreMasterRecord) => void;
}) {
  const tree = buildOperatingTree(records, query);
  const [collapsed, setCollapsed] = React.useState<Set<string>>(new Set());
  const byId = new Map(records.map((record) => [record.id, record]));
  const toggle = (id: string) =>
    setCollapsed((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  return (
    <div className="operating-tree" role="tree" aria-label="经营组织树">
      <header>
        <span>经营路径</span>
        <span>下级</span>
        <span>状态</span>
        <span>操作</span>
      </header>
      {tree.map((node) => (
        <OperatingTreeRow
          key={node.id}
          node={node}
          byId={byId}
          collapsed={collapsed}
          toggle={toggle}
          canManage={canManage}
          onEdit={onEdit}
          onStatus={onStatus}
          onDelete={onDelete}
        />
      ))}
    </div>
  );
}

function OperatingTreeRow({
  node,
  byId,
  collapsed,
  toggle,
  canManage,
  onEdit,
  onStatus,
  onDelete,
}: {
  node: OperatingUnitNode;
  byId: Map<string, CoreMasterRecord>;
  collapsed: Set<string>;
  toggle: (id: string) => void;
  canManage: boolean;
  onEdit: (record: CoreMasterRecord, detail?: boolean) => void;
  onStatus: (record: CoreMasterRecord) => void;
  onDelete: (record: CoreMasterRecord) => void;
}) {
  const record = byId.get(node.id);
  const isCollapsed = collapsed.has(node.id);
  const path = [...node.ancestorPath.slice(0, -1), node.name].join(" / ");
  return (
    <React.Fragment>
      <div
        role="treeitem"
        tabIndex={0}
        onClick={(event) => {
          if ((event.target as HTMLElement).closest("button")) return;
          if (record) onEdit(record, true);
        }}
        onKeyDown={(event) => {
          if (
            event.target !== event.currentTarget ||
            !["Enter", " "].includes(event.key)
          )
            return;
          event.preventDefault();
          if (record) onEdit(record, true);
        }}
        aria-expanded={node.children.length ? !isCollapsed : undefined}
        className={`${node.status === "disabled" ? "disabled" : ""} ${node.orphaned ? "orphan" : ""}`}
        style={{ "--tree-depth": node.depth } as React.CSSProperties}
      >
        <div className="operating-tree-name">
          <button
            type="button"
            className="operating-tree-toggle"
            disabled={node.children.length === 0}
            aria-label={isCollapsed ? "展开下级" : "收起下级"}
            onClick={() => toggle(node.id)}
          >
            {node.children.length === 0 ? "·" : isCollapsed ? "+" : "−"}
          </button>
          <button
            type="button"
            className="master-record-link"
            onClick={() => record && onEdit(record, true)}
            aria-label={`查看${node.name}详情`}
          >
            <code>{node.code}</code>
            <strong>{node.name}</strong>
            <small>{path || node.name}</small>
          </button>
        </div>
        <b>{node.descendantCount}</b>
        <span className={`master-status ${node.status}`}>
          {node.status === "active" ? "启用" : "停用"}
        </span>
        <div className="master-actions">
          {record && canManage && (
            <>
              <button type="button" onClick={() => onEdit(record)}>
                编辑
              </button>
              <button type="button" onClick={() => onStatus(record)}>
                {record.status === "active" ? "停用" : "启用"}
              </button>
              <button
                type="button"
                className="danger"
                onClick={() => onDelete(record)}
              >
                删除
              </button>
            </>
          )}
        </div>
      </div>
      {!isCollapsed &&
        node.children.map((child) => (
          <OperatingTreeRow
            key={child.id}
            node={child}
            byId={byId}
            collapsed={collapsed}
            toggle={toggle}
            canManage={canManage}
            onEdit={onEdit}
            onStatus={onStatus}
            onDelete={onDelete}
          />
        ))}
    </React.Fragment>
  );
}
