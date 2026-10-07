const PRODUCTION_HOSTNAMES = new Set([
  "business.shiyueshizi.com",
  "business.xiakeyuzhou.com",
]);

export function resolveBusinessEnvironmentLabel(
  configuredLabel: string | undefined,
  hostname: string,
): string {
  const configured = configuredLabel?.trim();
  if (configured) return configured;
  return PRODUCTION_HOSTNAMES.has(hostname) ? "Production" : "Staging";
}
