import React from "react";
import { CrmRelatedOrders } from "./CrmRelatedOrders";
import { SalesOrderEntry } from "./SalesOrderEntry";
import { useCrmDraft } from "./CrmDrawer";
import { request as read } from "./api";
import { formatMoney } from "./formatters";
import { CRM_STAGES, type CrmDetail as Detail } from "./crm";
export function CrmDetail({
  data,
  canManage,
  onEdit,
}: {
  data: Detail;
  canManage: boolean;
  onEdit: () => void;
}) {
  const draft = useCrmDraft();
  const item = data.item;
  const [orderEntry, setOrderEntry] = React.useState(false);
  const [orderSaved, setOrderSaved] = React.useState(false);
  const [orderRevision, setOrderRevision] = React.useState(0);
  const [history, setHistory] = React.useState(data.followups);
  const [hasOlder, setHasOlder] = React.useState(data.hasOlderFollowups);
  const [historyLoading, setHistoryLoading] = React.useState(false);
  const older = async () => {
    setHistoryLoading(true);
    setError("");
    try {
      const next = await read<Detail>(
        `/api/v1/crm/opportunities/${item.id}?offset=${history.length}`,
      );
      setHistory((previous) => [
        ...previous,
        ...next.followups.filter((n) => !previous.some((p) => p.id === n.id)),
      ]);
      setHasOlder(next.hasOlderFollowups);
    } catch (e) {
      setError(e instanceof Error ? e.message : "跟进记录加载失败");
    } finally {
      setHistoryLoading(false);
    }
  };
  const [error, setError] = React.useState("");
  const [busy, setBusy] = React.useState(false);
  if (orderEntry)
    return (
      <div
        className="crm-sales-entry"
        onChangeCapture={() => draft.markDirty()}
      >
        <button
          disabled={busy}
          onClick={() => draft.discard(() => setOrderEntry(false))}
        >
          返回商机
        </button>
        <SalesOrderEntry
          opportunityId={item.id}
          onBusy={(value) => {
            setBusy(value);
            draft.setBusy(value);
          }}
          onDone={() => {
            draft.saved();
            setOrderEntry(false);
            setOrderSaved(true);
            setOrderRevision((v) => v + 1);
          }}
        />
      </div>
    );
  return (
    <div className="crm-detail">
      <div className="crm-heading">
        <div>
          <span className={`crm-stage crm-stage-${item.stage}`}>
            {CRM_STAGES[item.stage]}
          </span>
          <h2>{item.title}</h2>
          <p>{item.companyName}</p>
        </div>
        {canManage && (
          <button
            onClick={() => {
              draft.discard(onEdit);
            }}
          >
            编辑商机
          </button>
        )}
      </div>
      <dl className="crm-facts">
        <div>
          <dt>商机负责人</dt>
          <dd>{item.ownerName || "未设置"}</dd>
        </div>
        <div>
          <dt>预计成交日期</dt>
          <dd>{item.expectedCloseDate || "未安排"}</dd>
        </div>
        <div>
          <dt>联系人</dt>
          <dd>{item.contactName || "未填写"}</dd>
        </div>
        <div>
          <dt>联系方式</dt>
          <dd>{item.contactDetails || "未填写"}</dd>
        </div>
        <div>
          <dt>预计金额</dt>
          <dd>
            {item.expectedAmountMinor == null
              ? "待确认"
              : formatMoney(item.currency, item.expectedAmountMinor / 100)}
          </dd>
        </div>
      </dl>
      {item.stage === "lost" && (
        <div className="crm-next">
          <strong>流失原因</strong>
          <p>{item.lossReason || "历史记录未填写"}</p>
        </div>
      )}

      {orderSaved && (
        <p role="status" className="crm-notice">
          销售订单草稿已保存，可前往<a href="/#sales">销售订单</a>查看。
        </p>
      )}
      {item.stage === "won" && (
        <p className="crm-hint">
          {canManage && item.customerId ? (
            <button
              className="primary"
              onClick={() => draft.discard(() => setOrderEntry(true))}
            >
              创建销售订单草稿
            </button>
          ) : (
            <>
              已成交。需要录单时，请前往<a href="#sales">销售订单</a>。
            </>
          )}
        </p>
      )}
      {item.customerId && (
        <CrmRelatedOrders opportunityId={item.id} revision={orderRevision} />
      )}
      {error && (
        <p role="alert" className="crm-error">
          {error}
        </p>
      )}

      <section className="crm-history">
        <h3>
          跟进记录 <span>{history.length}</span>
        </h3>
        {!history.length && <p className="crm-hint">还没有跟进记录。</p>}
        {history.map((note) => (
          <article key={note.id}>
            <div className="crm-note-meta">
              <strong>{note.authorName}</strong>
              <time>{new Date(note.createdAt).toLocaleString("zh-CN")}</time>
              <span>{CRM_STAGES[note.stage]}</span>
            </div>
            <p>{note.note}</p>
            {note.lossReason && (
              <p className="crm-hint">流失原因：{note.lossReason}</p>
            )}
            {note.nextAction && (
              <p className="crm-hint">
                下一步：{note.nextAction}
                {note.nextFollowUp && ` · ${note.nextFollowUp}`}
              </p>
            )}
          </article>
        ))}
        {hasOlder && (
          <button disabled={historyLoading} onClick={older}>
            {historyLoading ? "加载中…" : "查看更早的跟进"}
          </button>
        )}
      </section>
    </div>
  );
}
