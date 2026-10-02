import type { SalesOrderLineDraft } from "./salesOrderEntryDraft";
export type SalesOrderDraftOptions = {
  canUpdate: boolean;
  draft: {
    id: string;
    orderNumber: string;
    legalEntityId: string;
    customerId: string;
    businessUnitId: string;
    departmentId: string | null;
    brandId: string | null;
    currency: string;
    paymentTermsDays: number;
    orderDate: string;
    requestedDeliveryDate: string | null;
    customerReference: string | null;
    businessNote: string | null;
    lifecycleStatus: string;
    version: number;
    lines: (Omit<SalesOrderLineDraft, "key" | "warehouseId"> & {
      warehouseId: string | null;
    })[];
  };
};
