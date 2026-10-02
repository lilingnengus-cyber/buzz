import type { FormEvent, ReactNode } from "react";
import { useOrderValidation } from "./OrderValidation";
import "./order-entry-responsive.css";

export function ValidatedMasterForm({ className, onSubmit, children }: {
  className: string;
  onSubmit: (event: FormEvent<HTMLFormElement>) => void;
  children: ReactNode;
}) {
  const validation = useOrderValidation();
  return <form className={className} noValidate
    onSubmit={(event) => {
      event.preventDefault();
      if (validation.validate(event.currentTarget)) onSubmit(event);
    }}
    onInput={validation.clear} onChange={validation.clear}
    onClick={(event) => {
      if ((event.target as HTMLElement).closest(".master-tree-choice, [role=option]")) validation.clear();
    }}>
    {validation.summary}
    {children}
  </form>;
}
