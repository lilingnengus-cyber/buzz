import { useEffect, useState } from "react";
import { request } from "./api";
import type { CrmAccount, CrmContact } from "./crm";
import { CrmDrawer } from "./CrmDrawer";
import { CrmDirectoryDetail } from "./CrmRecordDetails";

export function CoreCustomerContacts({ customerId, customerName }: {
  customerId: string;
  customerName: string;
}) {
  const [contacts, setContacts] = useState<CrmContact[]>([]);
  const [selected, setSelected] = useState<CrmContact | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [revision, setRevision] = useState(0);
  useEffect(() => {
    const controller = new AbortController();
    async function pages<T>(path: string): Promise<T[]> {
      const items: T[] = [];
      for (let offset = 0; offset <= 100000; offset += 50) {
        const page = await request<{ items: T[]; hasMore?: boolean }>(
          `${path}&offset=${offset}`, { signal: controller.signal },
        );
        items.push(...page.items);
        if (!page.hasMore) return items;
      }
      throw new Error("关联记录过多，请前往联系人页面查询");
    }
    setLoading(true);
    setError("");
    setSelected(null);
    async function load() {
      const accounts = await pages<CrmAccount>(
        `/api/v1/crm/accounts?query=${encodeURIComponent(customerName.slice(0, 160))}`,
      );
      const related = accounts.filter((account) => account.customerId === customerId);
      const groups = await Promise.all(related.map((account) => pages<CrmContact>(
        `/api/v1/crm/contacts?accountId=${encodeURIComponent(account.id)}`,
      )));
      if (!controller.signal.aborted)
        setContacts(groups.flat().filter((contact) => contact.customerId === customerId));
    }
    load().catch((reason) => {
      if (!controller.signal.aborted)
        setError(reason instanceof Error ? reason.message : "联系人读取失败");
    }).finally(() => {
      if (!controller.signal.aborted) setLoading(false);
    });
    return () => controller.abort();
  }, [customerId, customerName, revision]);
  return (
    <section className="crm-related" aria-label="关联联系人">
      <h3>关联联系人</h3>
      {loading ? <p role="status">正在读取联系人…</p> : error ? (
        <div role="alert"><p>{error}</p><button type="button" onClick={() => setRevision((value) => value + 1)}>重试</button></div>
      ) : contacts.length ? contacts.map((contact) => (
        <button type="button" key={contact.id} onClick={() => setSelected(contact)}>
          {contact.contactName} · {contact.contactDetails || "联系方式未填写"}
        </button>
      )) : <p className="crm-hint">尚未关联联系人</p>}
      {selected && <CrmDrawer title="联系人详情" onClose={() => setSelected(null)}>
        <CrmDirectoryDetail item={selected} canManage={false} onEdit={() => {}} onContact={setSelected} />
      </CrmDrawer>}
    </section>
  );
}
