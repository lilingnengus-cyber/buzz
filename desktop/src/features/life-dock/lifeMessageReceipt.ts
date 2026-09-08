import { parseTrustedLifeExtensionResult } from "./lifeLinkHandler";

/** Decode display-only metadata. Failure receipts never become navigable results. */
export function lifeMessageReceipt(
  tags: readonly (readonly string[])[],
): string | null {
  const result = parseTrustedLifeExtensionResult(tags);
  if (result) {
    return [
      `${result.operation} succeeded`,
      `Trace ID: ${result.traceId}`,
      `Audit ID: ${result.auditId}`,
      ...tags
        .filter((tag) => tag[0] === "pacioli-resource-ref")
        .map((tag) => `${tag[3]}${tag[4] ? ` v${tag[4]}` : ""}`),
    ].join("\n");
  }
  const markers = tags.filter((tag) => tag[0] === "pacioli-extension-result");
  if (
    markers.length !== 1 ||
    tags.some((tag) => tag[0] === "pacioli-resource-ref")
  )
    return null;
  const marker = markers[0];
  // Reuse the same strict operation/UUID validator, without authorizing any
  // resource navigation or presenting this failure as a successful result.
  if (marker.length !== 7 || marker[4] !== "failed" || marker[6] !== "")
    return null;
  const validation = [...marker];
  validation[4] = "succeeded";
  validation[6] = marker[5];
  if (!parseTrustedLifeExtensionResult([validation])) return null;
  return `${marker[3]} failed\nTrace ID: ${marker[5]}`;
}
