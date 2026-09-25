import "./authority-assignment-pair.css";

export function AuthorityAssignmentPair({
  legalEntityId,
  legalEntityIds,
  businessUnitId,
  businessUnitIds,
  legalEntityFallback = "未指定法定主体",
  businessUnitFallback = "待核销归属",
  compact = false,
}: {
  legalEntityId?: string;
  legalEntityIds?: string[];
  businessUnitId?: string;
  businessUnitIds?: string[];
  legalEntityFallback?: string;
  businessUnitFallback?: string;
  compact?: boolean;
}) {
  const operatingUnits =
    businessUnitIds ?? (businessUnitId ? [businessUnitId] : []);
  const legalEntities =
    legalEntityIds ?? (legalEntityId ? [legalEntityId] : []);
  return (
    <div
      className={`authority-assignment-pair${compact ? " compact" : ""}`}
      role="group"
      aria-label="法定主体与经营单元独立归属"
    >
      <Assignment
        label={
          legalEntities.length > 1
            ? `法定主体 · ${legalEntities.length} 项`
            : "法定主体"
        }
        values={legalEntities}
        fallback={legalEntityFallback}
        tone="legal"
      />
      <Assignment
        label={
          operatingUnits.length > 1
            ? `经营单元 · ${operatingUnits.length} 项`
            : "经营单元"
        }
        values={operatingUnits}
        fallback={businessUnitFallback}
        tone="operating"
      />
    </div>
  );
}

function Assignment({
  label,
  values,
  fallback,
  tone,
}: {
  label: string;
  values: string[];
  fallback?: string;
  tone: "legal" | "operating";
}) {
  return (
    <div className={`authority-assignment ${tone}`}>
      <small>{label}</small>
      <div className="authority-assignment-values">
        {values.length > 0 ? (
          values.map((value) => (
            <code key={value} title={value}>
              {value}
            </code>
          ))
        ) : (
          <span>{fallback}</span>
        )}
      </div>
    </div>
  );
}
