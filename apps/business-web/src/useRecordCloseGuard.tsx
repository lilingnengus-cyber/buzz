import { useEffect, useRef, useState } from "react";
import { DiscardPrompt } from "./CrmDrawer";

export function useRecordCloseGuard(value: unknown, enabled: boolean, busy: boolean, onClose: () => void) {
  const signature = JSON.stringify(value);
  const baseline = useRef(signature);
  const [pending, setPending] = useState(false);
  const dirty = enabled && baseline.current !== signature;
  useEffect(() => {
    const guard = (event: BeforeUnloadEvent) => {
      if (dirty || busy) { event.preventDefault(); event.returnValue = ""; }
    };
    window.addEventListener("beforeunload", guard);
    return () => window.removeEventListener("beforeunload", guard);
  }, [dirty, busy]);
  return {
    close: () => {
      if (busy) return;
      if (dirty) setPending(true);
      else onClose();
    },
    saved: () => { baseline.current = signature; setPending(false); },
    prompt: pending && <DiscardPrompt onCancel={() => setPending(false)} onDiscard={() => { setPending(false); onClose(); }} />,
  };
}
