import {
  readWorkbenchAuthConfig,
  type WorkbenchAuthConfig,
} from "@/features/workbench-auth/workbenchAuthConfig";

export type LifeAuthEnv = {
  VITE_LIFE_OIDC_ISSUER?: string;
  VITE_LIFE_OIDC_CLIENT_ID?: string;
  VITE_LIFE_OIDC_REDIRECT_URI?: string;
  VITE_LIFE_OIDC_POST_LOGOUT_REDIRECT_URI?: string;
  VITE_LIFE_OIDC_DESKTOP_PROXY_ORIGIN?: string;
};

export type LifeAuthConfigResult =
  | { config: WorkbenchAuthConfig; error: null }
  | { config: null; error: string | null };

export function readLifeAuthConfig(env: LifeAuthEnv): LifeAuthConfigResult {
  const result = readWorkbenchAuthConfig({
    VITE_OIDC_ISSUER: env.VITE_LIFE_OIDC_ISSUER,
    VITE_OIDC_CLIENT_ID: env.VITE_LIFE_OIDC_CLIENT_ID,
    VITE_OIDC_REDIRECT_URI: env.VITE_LIFE_OIDC_REDIRECT_URI,
    VITE_OIDC_POST_LOGOUT_REDIRECT_URI:
      env.VITE_LIFE_OIDC_POST_LOGOUT_REDIRECT_URI,
    VITE_OIDC_DESKTOP_PROXY_ORIGIN: env.VITE_LIFE_OIDC_DESKTOP_PROXY_ORIGIN,
  });
  return result.error
    ? {
        config: null,
        error: result.error
          .replaceAll("Workbench OIDC", "Life OIDC")
          .replaceAll("VITE_OIDC_", "VITE_LIFE_OIDC_"),
      }
    : result;
}

export function getLifeAuthConfig(): LifeAuthConfigResult {
  return readLifeAuthConfig(import.meta.env);
}

/** Map the fixed desktop handoff back to the registered HTTPS redirect URI. */
export function normalizeLifeAuthCallback(
  url: string,
  config: WorkbenchAuthConfig,
): string {
  const target = new URL(config.redirectUri);
  const actual = new URL(url);
  if (
    target.href === "https://life.shiyueshizi.com/auth/pacioli" &&
    actual.protocol === "pacioli:" &&
    actual.host === "auth" &&
    actual.pathname === "/life-callback" &&
    !actual.username &&
    !actual.password &&
    !actual.hash
  ) {
    target.search = actual.search;
    return target.href;
  }
  return url;
}
