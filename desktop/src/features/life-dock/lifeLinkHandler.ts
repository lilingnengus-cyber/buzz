import type { RelayEvent } from "../../shared/api/types";
import type { WorkspaceResource } from "../workspace-dock/workspaceDockTypes";
import { resolveLifeResource } from "./lifeResourceResolver";

export const TRUSTED_LIFE_RESULT_CANDIDATE_EVENT =
  "buzz:trusted-life-result-candidate";

const RESULT_TAG = "pacioli-extension-result";
const RESOURCE_TAG = "pacioli-resource-ref";
const UUID_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/u;
const OPERATION_PATTERN = /^[a-z][a-z0-9._]{0,127}$/u;

export type TrustedLifeExtensionResult = {
  operation: string;
  traceId: string;
  auditId: string;
  resourceRefs: WorkspaceResource[];
};

function safeTitle(value: string): boolean {
  return (
    value.length <= 256 &&
    value.trim() === value &&
    !Array.from(value).some((character) => {
      const codePoint = character.codePointAt(0);
      return (
        codePoint !== undefined && (codePoint <= 0x1f || codePoint === 0x7f)
      );
    })
  );
}

/** Parses only ACP-produced, signed Life extension-result tags. */
export function parseTrustedLifeExtensionResult(
  tags: readonly (readonly string[])[],
): TrustedLifeExtensionResult | null {
  const markers = tags.filter((tag) => tag[0] === RESULT_TAG);
  if (markers.length !== 1) return null;
  const marker = markers[0];
  if (
    marker.length !== 7 ||
    marker[1] !== "1" ||
    marker[2] !== "life" ||
    !OPERATION_PATTERN.test(marker[3]) ||
    marker[4] !== "succeeded" ||
    !UUID_PATTERN.test(marker[5]) ||
    !UUID_PATTERN.test(marker[6])
  ) {
    return null;
  }
  const references = tags.filter((tag) => tag[0] === RESOURCE_TAG);
  if (references.length > 100) return null;
  const resourceRefs: WorkspaceResource[] = [];
  for (const tag of references) {
    if (
      tag.length !== 6 ||
      tag[1] !== "1" ||
      tag[2] !== marker[5] ||
      (tag[4] !== "" &&
        (!/^[1-9]\d{0,15}$/u.test(tag[4]) ||
          !Number.isSafeInteger(Number(tag[4])))) ||
      !safeTitle(tag[5])
    ) {
      return null;
    }
    const resource = resolveLifeResource(tag[3]);
    if (!resource) return null;
    resourceRefs.push(tag[5] ? { ...resource, title: tag[5] } : resource);
  }
  return {
    operation: marker[3],
    traceId: marker[5],
    auditId: marker[6],
    resourceRefs,
  };
}

export function lifeLinkAction(
  href: string | undefined,
  modifierPressed: boolean,
): { action: "dock" | "browser"; resource: WorkspaceResource } | null {
  const resource = href ? resolveLifeResource(href) : null;
  return resource
    ? { action: modifierPressed ? "browser" : "dock", resource }
    : null;
}

export function dispatchTrustedLifeResultCandidate(event: RelayEvent): void {
  if (
    event.kind !== 9 ||
    !event.tags.some((tag) => tag[0] === RESULT_TAG) ||
    typeof window === "undefined"
  ) {
    return;
  }
  window.dispatchEvent(
    new CustomEvent(TRUSTED_LIFE_RESULT_CANDIDATE_EVENT, {
      detail: {
        eventId: event.id,
        signerPubkey: event.pubkey,
        tags: event.tags,
      },
    }),
  );
}
