import * as React from "react";
import { toast } from "sonner";

import { useKnownAgentPubkeys } from "../agents/useKnownAgentPubkeys";
import { normalizePubkey } from "../../shared/lib/pubkey";
import { useLifeDock } from "./LifeDockProvider";
import {
  parseTrustedLifeExtensionResult,
  TRUSTED_LIFE_RESULT_CANDIDATE_EVENT,
} from "./lifeLinkHandler";

type Candidate = {
  eventId: string;
  signerPubkey: string;
  tags: string[][];
};

function isCandidate(value: unknown): value is Candidate {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Partial<Candidate>;
  return (
    typeof candidate.eventId === "string" &&
    typeof candidate.signerPubkey === "string" &&
    Array.isArray(candidate.tags)
  );
}

export function LifeTrustedTurnFollower() {
  const knownAgentPubkeys = useKnownAgentPubkeys();
  const { openLifeResourceAutomatically } = useLifeDock();
  const seenRef = React.useRef(new Set<string>());

  React.useEffect(() => {
    const onResult = (event: Event) => {
      if (!(event instanceof CustomEvent) || !isCandidate(event.detail)) return;
      const { eventId, signerPubkey, tags } = event.detail;
      if (seenRef.current.has(eventId)) return;
      seenRef.current.add(eventId);
      if (seenRef.current.size > 256) {
        const oldest = seenRef.current.values().next().value;
        if (oldest) seenRef.current.delete(oldest);
      }
      if (!knownAgentPubkeys.has(normalizePubkey(signerPubkey))) return;
      const result = parseTrustedLifeExtensionResult(tags);
      const resource = result?.resourceRefs[0];
      if (!resource) return;
      if (!openLifeResourceAutomatically(resource)) {
        toast.info(
          "A verified LifeOS result is available. Open it from the message link.",
        );
      }
    };
    window.addEventListener(TRUSTED_LIFE_RESULT_CANDIDATE_EVENT, onResult);
    return () =>
      window.removeEventListener(TRUSTED_LIFE_RESULT_CANDIDATE_EVENT, onResult);
  }, [knownAgentPubkeys, openLifeResourceAutomatically]);

  return null;
}
