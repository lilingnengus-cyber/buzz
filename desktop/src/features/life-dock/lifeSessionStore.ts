import { invoke, isTauri } from "@tauri-apps/api/core";
import type { LifeWorkbenchSession } from "./lifeAuthGateway";

/** Life credentials remain in the OS secret store, never browser localStorage. */
export function lifeSessionStore(gateway: string, pubkey: string) {
  const key = `buzz.life-workbench.session:${gateway}:${pubkey}`;
  return {
    async load(): Promise<LifeWorkbenchSession | null> {
      if (!isTauri()) return null;
      const raw = await invoke<string | null>("workbench_oidc_user_load", {
        key,
      });
      if (!raw) return null;
      try {
        const value = JSON.parse(raw);
        return typeof value.sessionToken === "string" &&
          /^[A-Za-z0-9_-]{43,128}$/.test(value.sessionToken) &&
          typeof value.expiresAt === "string" &&
          Number.isFinite(Date.parse(value.expiresAt))
          ? value
          : null;
      } catch {
        return null;
      }
    },
    async save(session: LifeWorkbenchSession) {
      if (isTauri())
        await invoke("workbench_oidc_user_save", {
          key,
          value: JSON.stringify(session),
        });
    },
    async clear() {
      if (isTauri()) await invoke("workbench_oidc_user_delete", { key });
    },
  };
}
