import React from "react";
import {
  buildOperatingTree,
  type OperatingUnitNode,
  type OperatingUnitRecord,
} from "./OperatingUnitTree";
import "./operating-unit-picker.css";

export function OperatingUnitPicker({
  label,
  records,
  value,
  onChange,
  disabled = false,
  allowEmpty = false,
  emptyLabel = "设为根节点",
}: {
  label: string;
  records: OperatingUnitRecord[];
  value: string;
  onChange: (value: string) => void;
  disabled?: boolean;
  allowEmpty?: boolean;
  emptyLabel?: string;
}) {
  const labelId = React.useId();
  const pickerRef = React.useRef<HTMLDivElement>(null);
  const [query, setQuery] = React.useState("");
  const tree = React.useMemo(
    () => buildOperatingTree(records, query),
    [records, query],
  );
  const [collapsed, setCollapsed] = React.useState<Set<string>>(new Set());
  const selected = records.find((record) => record.id === value);
  const selectedPath = selected
    ? [...(selected.ancestorPath ?? []).slice(0, -1), selected.name].join(" / ")
    : "尚未选择";
  const toggle = (id: string) =>
    setCollapsed((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  const moveChoiceFocus = (
    current: HTMLButtonElement | null,
    direction: -1 | 1,
  ) => {
    const choices = Array.from(
      pickerRef.current?.querySelectorAll<HTMLButtonElement>(
        ".master-tree-option.root:not(:disabled), .master-tree-choice:not(:disabled)",
      ) ?? [],
    );
    if (choices.length === 0) return;
    const currentIndex = current
      ? choices.indexOf(current)
      : direction === 1
        ? -1
        : choices.length;
    const nextIndex = Math.max(
      0,
      Math.min(choices.length - 1, currentIndex + direction),
    );
    choices[nextIndex]?.focus();
  };

  return (
    <div className="master-tree-field wide">
      <span id={labelId}>{label}</span>
      <div
        ref={pickerRef}
        className={`master-tree-picker ${disabled ? "disabled" : ""}`}
      >
        <div className="master-tree-selection" aria-live="polite">
          <small>当前选择</small>
          <strong>{selectedPath}</strong>
          {selected && <code>{selected.code}</code>}
        </div>
        <div className="master-tree-search">
          <input
            type="search"
            aria-label={`${label}搜索`}
            placeholder="搜索名称或编码"
            value={query}
            disabled={disabled}
            onChange={(event) => setQuery(event.target.value)}
            onKeyDown={(event) => {
              if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
              event.preventDefault();
              moveChoiceFocus(null, event.key === "ArrowDown" ? 1 : -1);
            }}
          />
        </div>
        <div role="tree" aria-labelledby={labelId}>
          {allowEmpty && (
            <button
              type="button"
              role="treeitem"
              aria-selected={!value}
              className={`master-tree-option root ${!value ? "selected" : ""}`}
              disabled={disabled}
              onClick={() => onChange("")}
              onKeyDown={(event) => {
                if (event.key !== "ArrowDown" && event.key !== "ArrowUp") {
                  return;
                }
                event.preventDefault();
                moveChoiceFocus(
                  event.currentTarget,
                  event.key === "ArrowDown" ? 1 : -1,
                );
              }}
            >
              <span>ROOT</span>
              <b>{emptyLabel}</b>
            </button>
          )}
          {tree.map((node) => (
            <OperatingUnitPickerNode
              key={node.id}
              node={node}
              value={value}
              collapsed={collapsed}
              disabled={disabled}
              onChange={onChange}
              onToggle={toggle}
              onNavigate={moveChoiceFocus}
              searching={Boolean(query.trim())}
            />
          ))}
          {tree.length === 0 && (
            <p className="master-tree-empty">没有匹配的经营主体</p>
          )}
        </div>
      </div>
    </div>
  );
}

function OperatingUnitPickerNode({
  node,
  value,
  collapsed,
  disabled,
  onChange,
  onToggle,
  onNavigate,
  searching,
}: {
  node: OperatingUnitNode;
  value: string;
  collapsed: Set<string>;
  disabled: boolean;
  onChange: (value: string) => void;
  onToggle: (id: string) => void;
  onNavigate: (current: HTMLButtonElement | null, direction: -1 | 1) => void;
  searching: boolean;
}) {
  const isCollapsed = !searching && collapsed.has(node.id);
  const selectable = node.id !== "__orphans__" && node.status === "active";
  return (
    <React.Fragment>
      <div
        role="treeitem"
        tabIndex={-1}
        aria-expanded={node.children.length ? !isCollapsed : undefined}
        aria-selected={value === node.id}
        className={`master-tree-option ${value === node.id ? "selected" : ""}`}
        style={{ "--tree-depth": node.depth } as React.CSSProperties}
      >
        <button
          type="button"
          className="master-tree-branch"
          aria-label={isCollapsed ? `展开 ${node.name}` : `收起 ${node.name}`}
          disabled={node.children.length === 0}
          onClick={() => onToggle(node.id)}
        >
          {node.children.length === 0 ? "·" : isCollapsed ? "+" : "−"}
        </button>
        <button
          type="button"
          className="master-tree-choice"
          disabled={disabled || !selectable}
          onClick={() => onChange(node.id)}
          onKeyDown={(event) => {
            if (event.key === "ArrowDown" || event.key === "ArrowUp") {
              event.preventDefault();
              onNavigate(
                event.currentTarget,
                event.key === "ArrowDown" ? 1 : -1,
              );
              return;
            }
            if (searching || node.children.length === 0) return;
            if (event.key === "ArrowLeft" && !isCollapsed) {
              event.preventDefault();
              onToggle(node.id);
            } else if (event.key === "ArrowRight" && isCollapsed) {
              event.preventDefault();
              onToggle(node.id);
            }
          }}
        >
          <code>{node.code}</code>
          <span>{node.name}</span>
        </button>
      </div>
      {!isCollapsed &&
        node.children.map((child) => (
          <OperatingUnitPickerNode
            key={child.id}
            node={child}
            value={value}
            collapsed={collapsed}
            disabled={disabled}
            onChange={onChange}
            onToggle={onToggle}
            onNavigate={onNavigate}
            searching={searching}
          />
        ))}
    </React.Fragment>
  );
}
