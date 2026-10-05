export type DecimalValue = string | number | null | undefined;

function numeric(value: DecimalValue) {
  if (value === null || value === undefined || value === "") return null;
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
}

export function formatDecimal(value: DecimalValue, fallback = "—") {
  const parsed = numeric(value);
  return parsed === null
    ? fallback
    : parsed.toLocaleString("zh-CN", {
        minimumFractionDigits: 2,
        maximumFractionDigits: 2,
      });
}

export const formatAmount = formatDecimal;
export const formatQuantity = formatDecimal;

const currencyNames = new Intl.DisplayNames(["zh-CN"], { type: "currency" });

export function formatCurrency(currency: string | null | undefined) {
  const code = currency?.trim().toUpperCase();
  if (!code) return "—";
  try {
    const name = currencyNames.of(code);
    return name && name !== code ? name : `未知币种（${code}）`;
  } catch {
    return `未知币种（${code}）`;
  }
}

export function formatMoney(currency: string, value: DecimalValue) {
  return `${formatCurrency(currency)} ${formatAmount(value)}`;
}

export function formatSignedQuantity(value: DecimalValue) {
  const parsed = numeric(value);
  if (parsed === null) return "—";
  return `${parsed > 0 ? "+" : ""}${formatQuantity(parsed)}`;
}

export function fixedDecimal(value: DecimalValue, fallback = "0.00") {
  const parsed = numeric(value);
  return parsed === null ? fallback : parsed.toFixed(2);
}
