import { isTauri } from "@tauri-apps/api/core";
import { getCurrent, onOpenUrl } from "@tauri-apps/plugin-deep-link";

import type { LifeDockConfig } from "./lifeDockConfig";

const LIFE_EMBED_CALLBACK = "pacioli://auth/life-bootstrap";
const EMBED_CODE_PATTERN = /^[A-Za-z0-9_-]{43}$/;

export type LifeEmbedSessionPhase =
  | "idle"
  | "authorizing"
  | "redeeming"
  | "ready"
  | "failed";

export function parseLifeEmbedCallback(value: string): string | null {
  try {
    const url = new URL(value);
    const expected = new URL(LIFE_EMBED_CALLBACK);
    if (
      url.protocol !== expected.protocol ||
      url.host !== expected.host ||
      url.pathname !== expected.pathname ||
      url.username ||
      url.password ||
      url.hash ||
      url.searchParams.size !== 1 ||
      [...url.searchParams.keys()].some((key) => key !== "code")
    ) {
      return null;
    }
    const code = url.searchParams.get("code");
    return code && EMBED_CODE_PATTERN.test(code) ? code : null;
  } catch {
    return null;
  }
}

export function buildLifeEmbedBootstrapUrl(
  config: LifeDockConfig,
  code: string,
): string | null {
  if (!EMBED_CODE_PATTERN.test(code)) return null;
  const target = new URL("/embed/bootstrap", config.origin);
  if (target.origin !== config.origin) return null;
  target.searchParams.set("code", code);
  return target.href;
}

export function validateLifeEmbedUrl(
  config: LifeDockConfig,
  value: string,
): string | null {
  try {
    const url = new URL(value);
    if (
      url.origin !== config.origin ||
      url.pathname !== "/embed/bootstrap" ||
      url.username ||
      url.password ||
      url.hash ||
      url.searchParams.size !== 1 ||
      [...url.searchParams.keys()].some((key) => key !== "code")
    ) {
      return null;
    }
    const code = url.searchParams.get("code");
    return code && EMBED_CODE_PATTERN.test(code) ? url.href : null;
  } catch {
    return null;
  }
}

export async function subscribeToLifeEmbedCallbacks(
  onCode: (code: string) => void,
): Promise<() => void> {
  if (!isTauri()) return () => undefined;
  const consumed = new Set<string>();
  const consume = (value: string) => {
    const code = parseLifeEmbedCallback(value);
    if (!code || consumed.has(code)) return;
    consumed.add(code);
    onCode(code);
  };
  for (const url of (await getCurrent()) ?? []) consume(url);
  return onOpenUrl((urls) => {
    for (const url of urls) consume(url);
  });
}

export function canAttemptLifeRecovery(attempts: number): boolean {
  return Number.isInteger(attempts) && attempts >= 0 && attempts < 1;
}
