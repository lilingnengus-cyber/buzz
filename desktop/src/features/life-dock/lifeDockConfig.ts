export type LifeDockConfig = {
  homeUrl: string;
  origin: string;
};

export type LifeDockConfigResult =
  | { enabled: false; config: null; error: null }
  | { enabled: true; config: LifeDockConfig; error: null }
  | { enabled: true; config: null; error: string };

export type LifeDockEnv = {
  LIFE_DOCK_ENABLED?: string;
  VITE_LIFE_APP_ORIGIN?: string;
  VITE_LIFE_APP_URL?: string;
};

function parseSwitch(value: string | undefined): boolean | null {
  if (value === undefined || value === "false") return false;
  if (value === "true") return true;
  return null;
}

function parseExactHttpOrigin(value: string): URL | null {
  try {
    const url = new URL(value);
    if (
      !["http:", "https:"].includes(url.protocol) ||
      url.username ||
      url.password ||
      url.pathname !== "/" ||
      url.search ||
      url.hash ||
      url.origin !== value
    ) {
      return null;
    }
    return url;
  } catch {
    return null;
  }
}

export function readLifeDockConfig(env: LifeDockEnv): LifeDockConfigResult {
  const enabled = parseSwitch(env.LIFE_DOCK_ENABLED);
  if (enabled === null) {
    return {
      enabled: true,
      config: null,
      error: "LIFE_DOCK_ENABLED must be true or false.",
    };
  }
  if (!enabled) return { enabled: false, config: null, error: null };

  const rawOrigin = env.VITE_LIFE_APP_ORIGIN?.trim();
  const rawHomeUrl = env.VITE_LIFE_APP_URL?.trim();
  if (!rawOrigin || !rawHomeUrl) {
    return {
      enabled: true,
      config: null,
      error: "Life workspace origin and home URL are required.",
    };
  }
  const origin = parseExactHttpOrigin(rawOrigin);
  if (!origin) {
    return {
      enabled: true,
      config: null,
      error: "Life workspace origin must be an exact HTTP(S) origin.",
    };
  }

  let homeUrl: URL;
  try {
    homeUrl = new URL(rawHomeUrl);
  } catch {
    return {
      enabled: true,
      config: null,
      error: "Life workspace home URL is invalid.",
    };
  }
  if (
    !["http:", "https:"].includes(homeUrl.protocol) ||
    homeUrl.username ||
    homeUrl.password ||
    homeUrl.origin !== origin.origin
  ) {
    return {
      enabled: true,
      config: null,
      error: "Life workspace home URL must use the configured origin.",
    };
  }

  return {
    enabled: true,
    config: { homeUrl: homeUrl.href, origin: origin.origin },
    error: null,
  };
}
