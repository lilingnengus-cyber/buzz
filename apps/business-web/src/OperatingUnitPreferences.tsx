import React from "react";
import { loadOperatingUnits } from "./operatingUnitOptions";
import { OperatingUnitPicker } from "./OperatingUnitPicker";
import {
  type AccountPreference,
  clearAccountOperatingUnitPreference,
  loadAccountOperatingUnitPreference,
  rememberRecentOperatingUnit,
  saveAccountOperatingUnitPreference,
} from "./recentOperatingUnit";
import "./operating-unit-preferences.css";

const CONTEXTS = [
  { id: "sales-order", label: "销售订单" },
  { id: "purchase-order", label: "采购订单" },
  { id: "crm-opportunity", label: "售前商机" },
] as const;

const INITIAL_CONTEXT = "sales-order";

type PreferenceContext = (typeof CONTEXTS)[number]["id"];

export function OperatingUnitPreferences() {
  const [units, setUnits] = React.useState<
    Awaited<ReturnType<typeof loadOperatingUnits>>
  >([]);
  const [preferences, setPreferences] = React.useState<
    Partial<Record<PreferenceContext, AccountPreference>>
  >({});
  const [context, setContext] =
    React.useState<PreferenceContext>(INITIAL_CONTEXT);
  const [draft, setDraft] = React.useState("");
  const [loading, setLoading] = React.useState(true);
  const [busy, setBusy] = React.useState(false);
  const [notice, setNotice] = React.useState("");
  const [error, setError] = React.useState("");

  React.useEffect(() => {
    let active = true;
    Promise.all([
      loadOperatingUnits(),
      Promise.all(
        CONTEXTS.map((item) => loadAccountOperatingUnitPreference(item.id)),
      ),
    ])
      .then(([nextUnits, nextPreferences]) => {
        if (!active) return;
        setUnits(nextUnits);
        setPreferences(
          Object.fromEntries(
            nextPreferences.map((preference) => [
              preference.context,
              preference,
            ]),
          ) as Partial<Record<PreferenceContext, AccountPreference>>,
        );
        setDraft(
          nextPreferences.find(
            (preference) => preference.context === INITIAL_CONTEXT,
          )?.businessUnitId ?? "",
        );
      })
      .catch((reason: unknown) => {
        if (active)
          setError(reason instanceof Error ? reason.message : "读取设置失败");
      })
      .finally(() => active && setLoading(false));
    return () => {
      active = false;
    };
  }, []);

  const selectContext = (next: PreferenceContext) => {
    setContext(next);
    setDraft(preferences[next]?.businessUnitId ?? "");
    setNotice("");
    setError("");
  };
  const current = preferences[context];
  const label = CONTEXTS.find((item) => item.id === context)?.label ?? context;

  const save = async () => {
    if (!draft) {
      setError("请先选择经营主体");
      return;
    }
    setBusy(true);
    setError("");
    setNotice("");
    try {
      const preference = await saveAccountOperatingUnitPreference(
        context,
        draft,
        true,
      );
      rememberRecentOperatingUnit(context, draft);
      setPreferences((items) => ({ ...items, [context]: preference }));
      setNotice(`${label}的默认经营主体已固定`);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "保存设置失败");
    } finally {
      setBusy(false);
    }
  };

  const clear = async () => {
    setBusy(true);
    setError("");
    setNotice("");
    try {
      const preference = await clearAccountOperatingUnitPreference(context);
      setPreferences((items) => ({ ...items, [context]: preference }));
      setDraft("");
      setNotice(`${label}已恢复使用业务默认值`);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "清除设置失败");
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="operating-preferences">
      <header className="operating-preferences-heading">
        <span>PERSONAL WORKSPACE</span>
        <h1>默认经营主体</h1>
        <p>设置会跟随当前登录账号同步；固定后不会被日常录单覆盖。</p>
      </header>
      {loading ? (
        <p className="operating-preferences-state">正在读取账号设置…</p>
      ) : error && units.length === 0 ? (
        <p className="operating-preferences-state error" role="alert">
          {error}
        </p>
      ) : (
        <div className="operating-preferences-layout">
          <nav aria-label="默认经营主体场景">
            {CONTEXTS.map((item) => {
              const preference = preferences[item.id];
              return (
                <button
                  type="button"
                  key={item.id}
                  className={context === item.id ? "active" : ""}
                  aria-current={context === item.id ? "page" : undefined}
                  onClick={() => selectContext(item.id)}
                >
                  <span>{item.label}</span>
                  <small>
                    {preference?.businessUnitId
                      ? preference.pinned
                        ? "已固定"
                        : "最近使用"
                      : "使用业务默认"}
                  </small>
                </button>
              );
            })}
          </nav>
          <div className="operating-preferences-editor">
            <div className="operating-preferences-current">
              <span>{label}</span>
              <strong>{preferencePath(current?.businessUnitId, units)}</strong>
              <small>
                {current?.pinned
                  ? "固定设置"
                  : current?.businessUnitId
                    ? "最近一次成功使用"
                    : "尚未设置"}
              </small>
            </div>
            <OperatingUnitPicker
              label={`${label}默认经营主体`}
              records={units}
              value={draft}
              onChange={setDraft}
              disabled={busy}
            />
            {(notice || error) && (
              <p
                className={`operating-preferences-notice ${error ? "error" : ""}`}
                role={error ? "alert" : "status"}
              >
                {error || notice}
              </p>
            )}
            <div className="operating-preferences-actions">
              <button
                type="button"
                className="primary"
                disabled={busy || !draft}
                onClick={save}
              >
                {busy ? "正在保存…" : "固定为默认值"}
              </button>
              <button
                type="button"
                disabled={busy || !current?.businessUnitId}
                onClick={clear}
              >
                清除设置
              </button>
            </div>
          </div>
        </div>
      )}
    </section>
  );
}

function preferencePath(
  id: string | null | undefined,
  units: Awaited<ReturnType<typeof loadOperatingUnits>>,
) {
  const unit = units.find((item) => item.id === id);
  if (!unit) return "使用业务默认值";
  return [...(unit.ancestorPath ?? []).slice(0, -1), unit.name].join(" / ");
}
