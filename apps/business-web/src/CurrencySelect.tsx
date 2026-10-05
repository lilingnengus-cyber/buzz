import type { SelectHTMLAttributes } from "react";
import { formatCurrency } from "./formatters";

export function CurrencySelect(props: SelectHTMLAttributes<HTMLSelectElement>) {
  const current = String(props.value ?? props.defaultValue ?? "CNY");
  const codes = Array.from(new Set(["CNY", "USD", "EUR", "HKD", "JPY", "GBP", "SGD", "AUD", "CAD", "CHF", ...Intl.supportedValuesOf("currency"), current]));
  return <select {...props}>{codes.map((code) => <option key={code} value={code}>{formatCurrency(code)}</option>)}</select>;
}
