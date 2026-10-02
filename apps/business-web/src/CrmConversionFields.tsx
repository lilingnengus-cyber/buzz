import React from "react";
import { request } from "./api";
import type { CrmOption, Opportunity } from "./crm";

export function CrmConversionFields({ item }: { item: Opportunity }) {
  const [customers, setCustomers] = React.useState<CrmOption[]>([]);
  const [selected, setSelected] = React.useState(item.customerId ?? "");
  const [error, setError] = React.useState("");
  React.useEffect(() => {
    const controller = new AbortController();
    request<{ items: CrmOption[] }>("/api/v1/crm/options", {
      signal: controller.signal,
    })
      .then((result) =>
        setCustomers(
          result.items.filter(
            (c) => c.resourceType === "customer" && c.status === "active",
          ),
        ),
      )
      .catch((reason) => {
        if (!controller.signal.aborted)
          setError(reason instanceof Error ? reason.message : "客户读取失败");
      });
    return () => controller.abort();
  }, []);
  return (
    <section className="crm-conversion">
      <h3>成交转客户</h3>
      <p className="crm-hint">
        确认正式客户和联系人资料后，一并保存成交与本次沟通。已有客户请直接选择，避免重复建档。
      </p>
      {error && (
        <p role="alert" className="crm-error">
          {error}
        </p>
      )}
      <label>
        正式客户
        <select
          aria-label="正式客户"
          name="conversionCustomerId"
          value={selected}
          disabled={!!item.customerId}
          onChange={(e) => setSelected(e.target.value)}
        >
          <option value="">新建正式客户</option>
          {customers.map((c) => (
            <option key={c.id} value={c.id}>
              {c.name} · {c.code}
            </option>
          ))}
          {item.customerId &&
            !customers.some((c) => c.id === item.customerId) && (
              <option value={item.customerId}>{item.companyName}</option>
            )}
        </select>
      </label>
      <input type="hidden" name="confirmedCustomerId" value={selected} />
      {!selected && (
        <>
          <label>
            正式客户名称
            <input
              name="conversionCustomerName"
              required
              maxLength={160}
              defaultValue={item.companyName}
            />
          </label>
          <div className="crm-fields">
            <label>
              信用币种
              <input
                name="conversionCurrency"
                required
                pattern="[A-Z]{3}"
                maxLength={3}
                defaultValue={item.currency}
              />
            </label>
            <label>
              付款账期（天）
              <input
                name="conversionTerms"
                type="number"
                required
                min={0}
                max={3650}
                defaultValue={30}
              />
            </label>
          </div>
          <p className="crm-hint">
            客户编码自动生成，初始信用额度为 0。新建客户需要核心数据维护权限。
          </p>
        </>
      )}
      <div className="crm-fields">
        <label>
          正式联系人姓名
          <input
            name="conversionContactName"
            required
            maxLength={100}
            defaultValue={item.contactName}
          />
        </label>
        <label>
          正式联系方式
          <input
            name="conversionContactDetails"
            required
            maxLength={200}
            defaultValue={item.contactDetails}
          />
        </label>
      </div>
      <p className="crm-hint">
        同一客户下姓名和联系方式相同的联系人将复用。其他商机和已有跟进历史保留。
      </p>
    </section>
  );
}
