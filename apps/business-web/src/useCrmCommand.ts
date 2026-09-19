import React from "react";
import { request } from "./api";
/** Reuse the command identity after an uncertain network result, until fields change. */
export function useCrmCommand() {
  const previous = React.useRef({ signature: "", key: "" });
  return <T>(path: string, init: RequestInit) => {
    const signature = JSON.stringify([path, init.method, init.body]);
    if (previous.current.signature !== signature)
      previous.current = { signature, key: crypto.randomUUID() };
    return request<T>(path, {
      ...init,
      headers: { "idempotency-key": previous.current.key },
    });
  };
}
