import React from "react";
import { request, toApiFailure, type ApiFailure } from "./api";
import { ShieldIcon } from "./OrderWorkflowIcons";
import { PageLoadFailure } from "./PageLoadFailure";
import type { WorkflowModalState as ModalState } from "./OrderWorkflowModal";
export function CommandConfirmation({
  state,
  onCancel,
  onDone,
}: {
  state: Extract<ModalState, { kind: "command" }>;
  onCancel: () => void;
  onDone: () => void;
}) {
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState("");
  return (
    <section
      className={`command-confirmation ${state.tone === "danger" ? "danger" : ""}`}
    >
      <div className="command-symbol">
        <ShieldIcon />
      </div>
      <h3>{state.title}</h3>
      <p>{state.description}</p>
      <dl>
        <div>
          <dt>控制方式</dt>
          <dd>版本校验 + 幂等命令</dd>
        </div>
        <div>
          <dt>记录方式</dt>
          <dd>审计日志与业务事实同步写入</dd>
        </div>
      </dl>
      {error && (
        <div className="workflow-inline-error" role="alert">
          {error}
        </div>
      )}
      <footer>
        <button type="button" className="secondary" onClick={onCancel}>
          取消
        </button>
        <button
          type="button"
          className={state.tone === "danger" ? "danger" : ""}
          disabled={busy}
          onClick={async () => {
            setBusy(true);
            setError("");
            try {
              await request(state.path, {
                method: "POST",
                body: JSON.stringify(state.body),
              });
              onDone();
            } catch (reason) {
              setError((reason as Error).message);
            } finally {
              setBusy(false);
            }
          }}
        >
          {busy ? "正在执行…" : state.confirmLabel}
        </button>
      </footer>
    </section>
  );
}

export function WorkflowError({
  error,
  resourceLabel,
  onRetry,
}: {
  error: ApiFailure;
  resourceLabel: string;
  onRetry: () => void;
}) {
  return (
    <PageLoadFailure
      failure={error}
      resourceLabel={resourceLabel}
      onRetry={onRetry}
    />
  );
}

export function useWorkflowData<T>(
  loader: () => Promise<T>,
  deps: React.DependencyList,
) {
  const [state, setState] = React.useState<{
    data: T | null;
    loading: boolean;
    error: ApiFailure | null;
  }>({ data: null, loading: true, error: null });
  React.useEffect(() => {
    let active = true;
    setState((current) => ({ ...current, loading: true, error: null }));
    loader()
      .then((data) => active && setState({ data, loading: false, error: null }))
      .catch(
        (error: unknown) =>
          active &&
          setState({
            data: null,
            loading: false,
            error: toApiFailure(error, "业务数据加载失败"),
          }),
      );
    return () => {
      active = false;
    };
    // biome-ignore lint/correctness/useExhaustiveDependencies: caller owns the explicit reload keys.
  }, deps);
  return state;
}
