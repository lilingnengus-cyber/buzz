import React from "react";
import {
  request,
  type CoreMasterRecord,
  type CoreMasterCommandResult,
} from "./api";
import { MasterModal } from "./CoreMasterDataCenter";

export function CoreMasterDeleteModal({
  record,
  onClose,
  onSaved,
}: {
  record: CoreMasterRecord;
  onClose: () => void;
  onSaved: () => Promise<void>;
}) {
  const [saving, setSaving] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const pending = React.useRef(false);
  const idempotencyKey = React.useRef(crypto.randomUUID());
  async function confirm() {
    if (pending.current) return;
    pending.current = true;
    setSaving(true);
    setError(null);
    try {
      await request<CoreMasterCommandResult>(
        `/api/v1/core-master-data/${record.resourceType}/${record.id}`,
        {
          method: "DELETE",
          headers: { "Idempotency-Key": idempotencyKey.current },
          body: JSON.stringify({ expectedVersion: record.version }),
        },
      );
      await onSaved();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "删除失败");
    } finally {
      pending.current = false;
      setSaving(false);
    }
  }
  return (
    <MasterModal
      title={`删除${{ legal_entity: "法定主体", business_unit: "经营主体", customer: "客户", supplier: "供应商", warehouse: "仓库" }[record.resourceType]}`}
      eyebrow="DELETE RECORD"
      busy={saving}
      onClose={() => {
        if (!pending.current) onClose();
      }}
    >
      <div className="impact-panel">
        <div className="impact-target">
          <code>{record.code}</code>
          <h3>{record.name}</h3>
          <p>
            删除后无法恢复。仅可删除未被下级数据或业务记录引用的记录；已有引用时请使用停用。
          </p>
        </div>
        {error && (
          <p className="master-form-error" role="alert">
            {error}
          </p>
        )}
        <div className="master-form-actions">
          <button
            type="button"
            className="master-secondary"
            disabled={saving}
            onClick={onClose}
          >
            取消
          </button>
          <button
            type="button"
            className="master-danger"
            disabled={saving}
            onClick={() => void confirm()}
          >
            {saving ? "删除中…" : "确认删除"}
          </button>
        </div>
      </div>
    </MasterModal>
  );
}
