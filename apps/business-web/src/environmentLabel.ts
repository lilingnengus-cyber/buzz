const PRODUCTION_HOSTNAME = "business.shiyueshizi.com";

export function resolveBusinessEnvironmentLabel(
  configuredLabel: string | undefined,
  hostname: string,
): string {
  const configured = configuredLabel?.trim();
  if (configured) return configured;
  return hostname === PRODUCTION_HOSTNAME ? "Production" : "Staging";
}
