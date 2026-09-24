import React from "react";
import "./authority-assignment-pair.css";

export function AuthorityAssignmentPair({
  legalEntityId,
  businessUnitId,
  compact = false,
}: {
  legalEntityId: string;
  businessUnitId: string;
  compact?: boolean;
}) {
  return (
    <div
      className={`authority-assignment-pair${compact ? " compact" : ""}`}
      aria-label="法定主体与经营单元独立归属"
    >
      <Assignment label="法定主体" value={legalEntityId} tone="legal" />
      <Assignment label="经营单元" value={businessUnitId} tone="operating" />
    </div>
  );
}

function Assignment({
  label,
  value,
  tone,
}: {
  label: string;
  value: string;
  tone: "legal" | "operating";
}) {
  return (
    <div className={`authority-assignment ${tone}`}>
      <small>{label}</small>
      <code title={value}>{value}</code>
    </div>
  );
}
