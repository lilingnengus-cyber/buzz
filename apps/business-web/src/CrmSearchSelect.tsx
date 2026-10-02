import React from "react";
import "./crm-search-select.css";

export function CrmSearchSelect({ label, value, options, query, onQuery, onChange, disabled = false, maxLength = 160 }: {
  label: string;
  value: string;
  options: { value: string; label: string }[];
  query: string;
  onQuery: (value: string) => void;
  onChange: (value: string) => void;
  disabled?: boolean;
  maxLength?: number;
}) {
  const [open, setOpen] = React.useState(false);
  const [active, setActive] = React.useState(0);
  const root = React.useRef<HTMLDivElement>(null);
  const trigger = React.useRef<HTMLButtonElement>(null);
  const search = React.useRef<HTMLInputElement>(null);
  const id = React.useId();
  const close = () => { setOpen(false); trigger.current?.focus(); };
  const choose = (next: string) => { onChange(next); onQuery(""); close(); };
  React.useEffect(() => {
    if (!open) return;
    search.current?.focus();
    const outside = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener("pointerdown", outside);
    return () => document.removeEventListener("pointerdown", outside);
  }, [open]);
  React.useEffect(() => { setActive(0); }, [query, options.length]);
  React.useEffect(() => { if (disabled) setOpen(false); }, [disabled]);
  return <div className="crm-search-select" ref={root}
    onBlur={(event) => { if (!event.currentTarget.contains(event.relatedTarget)) setOpen(false); }}
    onKeyDown={(event) => {
      if (open && event.key === "Escape") { event.preventDefault(); event.stopPropagation(); close(); }
    }}>
    <span id={`${id}-label`}>{label}</span>
    <button type="button" ref={trigger} role="combobox" aria-labelledby={`${id}-label`}
      aria-expanded={open} aria-controls={`${id}-list`} aria-haspopup="listbox" disabled={disabled}
      onClick={() => setOpen((current) => !current)}
      onKeyDown={(event) => { if (event.key === "ArrowDown") { event.preventDefault(); setOpen(true); } }}>
      <span>{options.find((option) => option.value === value)?.label ?? "请选择"}</span><span aria-hidden="true">▾</span>
    </button>
    {open && <div className="crm-search-select-panel">
      <input ref={search} type="search" aria-label={`搜索${label}`} placeholder="输入关键词搜索"
        value={query} maxLength={maxLength} aria-controls={`${id}-list`}
        role="combobox" aria-expanded="true" aria-autocomplete="list"
        aria-activedescendant={options[active] ? `${id}-${active}` : undefined}
        onChange={(event) => onQuery(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown" || event.key === "ArrowUp") {
            event.preventDefault();
            const next = Math.max(0, Math.min(options.length - 1, active + (event.key === "ArrowDown" ? 1 : -1)));
            setActive(next);
            document.getElementById(`${id}-${next}`)?.scrollIntoView({ block: "nearest" });
          }
          if (event.key === "Enter") { event.preventDefault(); if (options[active]) choose(options[active].value); }
        }} />
      <div id={`${id}-list`} role="listbox" aria-label={label}>
        {options.map((option, index) => <button type="button" role="option" id={`${id}-${index}`} key={option.value}
          aria-selected={option.value === value} className={index === active ? "is-active" : ""}
          onClick={() => choose(option.value)}>{option.label}</button>)}
        {options.length <= 1 && query && <p className="crm-hint">暂无匹配结果，可调整关键词</p>}
      </div>
    </div>}
  </div>;
}
