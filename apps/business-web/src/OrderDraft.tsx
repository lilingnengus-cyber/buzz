import React from "react";
import { useCrmDraft } from "./CrmDrawer";

export const OrderDraftContext = React.createContext<{
  markDirty: () => void;
  saved: () => void;
  setBusy: (busy: boolean) => void;
} | null>(null);

export function useOrderDraft() {
  const order = React.useContext(OrderDraftContext);
  const crm = useCrmDraft();
  return order ?? crm;
}
