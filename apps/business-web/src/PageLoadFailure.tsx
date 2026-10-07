import { useState } from "react";
import type { ApiFailure } from "./api";
import { requestBusinessSignIn } from "./businessDockBridge";
import "./page-load-failure.css";

const COPY: Record<
  ApiFailure["kind"],
  {
    action: string;
    title: (resourceLabel: string) => string;
    description: string;
  }
> = {
  access_denied: {
    action: "重新检查",
    title: (resourceLabel) => `当前账号无法访问${resourceLabel}`,
    description:
      "此账号未开通该页面，或其业务范围不包含相关数据。请联系企业管理员确认权限。",
  },
  session_expired: {
    action: "重新登录",
    title: () => "登录状态已失效",
    description: "为保护企业数据，请重新登录后继续访问当前页面。",
  },
  service_unavailable: {
    action: "重新加载",
    title: (resourceLabel) => `${resourceLabel}暂时不可用`,
    description: "服务暂时未能响应，请稍后重试。若持续发生，请联系系统管理员。",
  },
  unexpected: {
    action: "重新加载",
    title: (resourceLabel) => `${resourceLabel}加载失败`,
    description: "页面未能完成加载，请重试。",
  },
};

export function PageLoadFailure({
  failure,
  resourceLabel,
  onRetry,
}: {
  failure: ApiFailure;
  resourceLabel: string;
  onRetry: () => void;
}) {
  const [recoveryError, setRecoveryError] = useState<string | null>(null);
  const copy = COPY[failure.kind];
  const detail =
    failure.kind === "unexpected" ? failure.message : copy.description;
  const handleRecovery = () => {
    if (failure.kind === "session_expired") {
      if (window.parent !== window) {
        if (!requestBusinessSignIn())
          setRecoveryError("登录连接尚未就绪，请重新打开企业工作台后重试。");
      } else {
        const returnTo =
          window.location.pathname + window.location.search + window.location.hash;
        window.location.assign(
          `/api/auth/login?return_to=${encodeURIComponent(returnTo)}`,
        );
      }
      return;
    }
    onRetry();
  };

  return (
    <section
      className={`page-load-failure ${failure.kind}`}
      data-failure-kind={failure.kind}
      role="alert"
    >
      <svg
        aria-hidden="true"
        className="page-load-failure-icon"
        viewBox="0 0 24 24"
      >
        <path d="M12 3 4 6v5c0 5.2 3.4 8.6 8 10 4.6-1.4 8-4.8 8-10V6l-8-3Z" />
        <path d="M12 8v5M12 16.5v.1" />
      </svg>
      <div>
        <h2>{copy.title(resourceLabel)}</h2>
        <p>{detail}</p>
        {recoveryError && <p role="status">{recoveryError}</p>}
        {failure.traceId && <small>追踪号：{failure.traceId}</small>}
        <button type="button" onClick={handleRecovery}>
          {copy.action}
        </button>
      </div>
    </section>
  );
}
