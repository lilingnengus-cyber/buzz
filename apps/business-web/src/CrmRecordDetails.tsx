import { useEffect, useState } from "react";
import { request } from "./api";
import type { CrmAccount, CrmContact, Opportunity, Followup } from "./crm";
import { CRM_STAGES } from "./crm";

export type CrmFollowupRecord = Followup & {
  opportunityId: string;
  opportunityTitle: string;
  companyName: string;
  contactName: string;
};
export function CrmDirectoryDetail({
  item,
  canManage,
  onEdit,
  onContact,
}: {
  item: CrmAccount | CrmContact;
  canManage: boolean;
  onEdit: () => void;
  onContact: (contact: CrmContact) => void;
}) {
  const contact = "contactName" in item;
  return (
    <div className="crm-record-detail">
      <header className="crm-heading">
        <div>
          <h2>{contact ? item.contactName : item.name}</h2>
          <p className="crm-hint">
            {contact
              ? item.companyName
              : item.customerId
                ? "已关联核心客户"
                : "潜在客户"}
          </p>
        </div>
        {canManage && (contact || !item.customerId) && (
          <button onClick={onEdit}>
            {contact ? "编辑联系人" : "编辑客户"}
          </button>
        )}
      </header>
      {contact ? (
        <>
          <dl className="crm-facts">
            <div>
              <dt>所属客户</dt>
              <dd>{item.companyName}</dd>
            </div>
            <div>
              <dt>联系方式</dt>
              <dd>{item.contactDetails || "未填写"}</dd>
            </div>
          </dl>
          <section className="crm-related">
            <h3>关联商机</h3>
            {item.opportunities.length ? (
              item.opportunities.map((o) => (
                <a
                  key={o.id}
                  href={`/#crm?opportunity=${encodeURIComponent(o.id)}`}
                >
                  {o.title}
                </a>
              ))
            ) : (
              <p className="crm-hint">尚未关联商机</p>
            )}
          </section>
        </>
      ) : item.customerId ? (
        <p className="crm-hint">
          名称复用核心客户资料。<a href="/#coreData">前往核心数据维护</a>
        </p>
      ) : (
        <p className="crm-hint">客户档案可在商机和联系人中复用。</p>
      )}
      {!contact && (
        <CustomerRelations key={item.id} account={item} onContact={onContact} />
      )}
    </div>
  );
}

function CustomerRelations({
  account,
  onContact,
}: {
  account: CrmAccount;
  onContact: (contact: CrmContact) => void;
}) {
  const [contacts, setContacts] = useState<CrmContact[]>([]);
  const [opportunities, setOpportunities] = useState<Opportunity[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [revision, setRevision] = useState(0);
  useEffect(() => {
    const controller = new AbortController();
    async function pages<T>(path: string): Promise<T[]> {
      const items: T[] = [];
      for (let offset = 0; offset <= 100000; offset += 50) {
        const page = await request<{ items: T[]; hasMore?: boolean }>(
          `${path}&offset=${offset}`,
          { signal: controller.signal },
        );
        items.push(...page.items);
        if (!page.hasMore) return items;
      }
      throw new Error("关联记录过多，请在列表中查询");
    }
    setLoading(true);
    setError("");
    Promise.all([
      pages<CrmContact>(
        `/api/v1/crm/contacts?accountId=${encodeURIComponent(account.id)}`,
      ),
      pages<Opportunity>(
        `/api/v1/crm/opportunities?query=${encodeURIComponent(account.name)}`,
      ),
    ])
      .then(([people, deals]) => {
        if (controller.signal.aborted) return;
        setContacts(people.filter((c) => c.accountId === account.id));
        setOpportunities(deals.filter((o) => o.accountId === account.id));
      })
      .catch((reason) => {
        if (!controller.signal.aborted)
          setError(
            reason instanceof Error ? reason.message : "关联记录读取失败",
          );
      })
      .finally(() => {
        if (!controller.signal.aborted) setLoading(false);
      });
    return () => controller.abort();
  }, [account.id, account.name, revision]);
  if (loading)
    return (
      <p className="crm-hint" role="status">
        正在读取关联记录…
      </p>
    );
  if (error)
    return (
      <div role="alert">
        <p>{error}</p>
        <button onClick={() => setRevision((v) => v + 1)}>重试</button>
      </div>
    );
  return (
    <>
      <section className="crm-related">
        <h3>关联联系人</h3>
        {contacts.length ? (
          contacts.map((c) => (
            <button key={c.id} onClick={() => onContact(c)}>
              {c.contactName}
              {c.contactDetails ? ` · ${c.contactDetails}` : ""}
            </button>
          ))
        ) : (
          <p className="crm-hint">尚未添加联系人</p>
        )}
      </section>
      <section className="crm-related">
        <h3>关联商机</h3>
        {opportunities.length ? (
          opportunities.map((o) => (
            <a
              key={o.id}
              href={`/#crm?opportunity=${encodeURIComponent(o.id)}`}
            >
              {o.title} · {CRM_STAGES[o.stage]}
            </a>
          ))
        ) : (
          <p className="crm-hint">尚未关联商机</p>
        )}
      </section>
    </>
  );
}
export function CrmFollowupDetail({ item }: { item: CrmFollowupRecord }) {
  return (
    <div className="crm-record-detail">
      <header className="crm-heading">
        <div>
          <span className={`crm-stage crm-stage-${item.stage}`}>
            {CRM_STAGES[item.stage]}
          </span>
          <h2>{item.opportunityTitle}</h2>
          <p className="crm-hint">{item.companyName}</p>
        </div>
      </header>
      <dl className="crm-facts">
        <div>
          <dt>联系人</dt>
          <dd>{item.contactName || "未填写"}</dd>
        </div>
        <div>
          <dt>记录人</dt>
          <dd>{item.authorName}</dd>
        </div>
        <div>
          <dt>记录时间</dt>
          <dd>
            <time dateTime={item.createdAt}>
              {new Date(item.createdAt).toLocaleString("zh-CN")}
            </time>
          </dd>
        </div>
        <div>
          <dt>跟进日期</dt>
          <dd>{item.nextFollowUp || "未安排"}</dd>
        </div>
      </dl>
      <section>
        <h3>沟通内容</h3>
        <p className="crm-note-content">{item.note}</p>
        {item.lossReason && (
          <p className="crm-hint">流失原因：{item.lossReason}</p>
        )}
      </section>
      <div className="crm-next">
        <strong>下一步</strong>
        <p>{item.nextAction || "未安排"}</p>
      </div>
      <a href={`/#crm?opportunity=${encodeURIComponent(item.opportunityId)}`}>
        打开商机继续跟进
      </a>
    </div>
  );
}
