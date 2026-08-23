import * as React from "react";

import { BusinessIamAdminDialog } from "@/features/business-iam-admin/BusinessIamAdminDialog";

type BusinessIamAdminContextValue = {
  open: boolean;
  setOpen: (open: boolean) => void;
  toggle: () => void;
};

const BusinessIamAdminContext =
  React.createContext<BusinessIamAdminContextValue | null>(null);

export function BusinessIamAdminProvider({
  children,
}: {
  children: React.ReactNode;
}) {
  const [open, setOpen] = React.useState(false);
  const value = React.useMemo(
    () => ({ open, setOpen, toggle: () => setOpen((current) => !current) }),
    [open],
  );
  return (
    <BusinessIamAdminContext.Provider value={value}>
      {children}
      <BusinessIamAdminDialog open={open} onOpenChange={setOpen} />
    </BusinessIamAdminContext.Provider>
  );
}

export function useBusinessIamAdmin() {
  const value = React.useContext(BusinessIamAdminContext);
  if (!value)
    throw new Error(
      "useBusinessIamAdmin must be used within BusinessIamAdminProvider",
    );
  return value;
}
