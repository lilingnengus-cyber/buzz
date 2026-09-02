import { createRequire } from "node:module";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { loadEnv } from "vite";

const require = createRequire(import.meta.url);

function normalizeDockOrigin(name, value) {
  if (!value?.trim()) return null;
  const url = new URL(value.trim());
  if (
    !["http:", "https:"].includes(url.protocol) ||
    url.username ||
    url.password ||
    url.hostname.includes("*") ||
    url.pathname !== "/" ||
    url.search ||
    url.hash
  ) {
    throw new Error(
      `${name} must be an HTTP(S) origin without credentials, path, query, or fragment`,
    );
  }
  return url.origin;
}

export function normalizeBusinessOrigin(value) {
  return normalizeDockOrigin("VITE_BUSINESS_APP_ORIGIN", value);
}

export function normalizeLifeOrigin(value) {
  return normalizeDockOrigin("VITE_LIFE_APP_ORIGIN", value);
}

export function buildWorkspaceDockCsp(
  baseCsp,
  configuredBusinessOrigin,
  configuredLifeOrigin,
) {
  const origins = [
    normalizeBusinessOrigin(configuredBusinessOrigin),
    normalizeLifeOrigin(configuredLifeOrigin),
  ].filter(Boolean);
  const directives = baseCsp
    .split(";")
    .map((directive) => directive.trim())
    .filter(Boolean)
    .filter((directive) => !directive.startsWith("frame-src "));
  directives.push(
    `frame-src 'self'${origins.map((origin) => ` ${origin}`).join("")}`,
  );
  return directives.join("; ");
}

export function buildBusinessDockCsp(baseCsp, configuredOrigin) {
  return buildWorkspaceDockCsp(baseCsp, configuredOrigin);
}

function withBusinessDockConfig(args) {
  if (!new Set(["build", "dev"]).has(args[0])) {
    return args;
  }
  const tauriConfigPath = new URL(
    "../src-tauri/tauri.conf.json",
    import.meta.url,
  );
  const tauriConfig = JSON.parse(readFileSync(tauriConfigPath, "utf8"));
  const mode = args[0] === "build" ? "production" : "development";
  const viteEnv = loadEnv(mode, process.cwd(), "");
  const lifeEnabled =
    process.env.VITE_LIFE_DOCK_ENABLED ??
    viteEnv.VITE_LIFE_DOCK_ENABLED ??
    process.env.LIFE_DOCK_ENABLED ??
    viteEnv.LIFE_DOCK_ENABLED ??
    "false";
  const lifeOrigin =
    process.env.VITE_LIFE_APP_ORIGIN ?? viteEnv.VITE_LIFE_APP_ORIGIN;
  if (lifeEnabled !== "true" && lifeEnabled !== "false") {
    throw new Error("LIFE_DOCK_ENABLED must be true or false");
  }
  if (lifeEnabled === "true" && !lifeOrigin) {
    throw new Error(
      "VITE_LIFE_APP_ORIGIN is required when LIFE_DOCK_ENABLED=true",
    );
  }
  const csp = buildWorkspaceDockCsp(
    tauriConfig.app.security.csp,
    process.env.VITE_BUSINESS_APP_ORIGIN ?? viteEnv.VITE_BUSINESS_APP_ORIGIN,
    lifeEnabled === "true" ? lifeOrigin : undefined,
  );
  const override = JSON.stringify({ app: { security: { csp } } });
  const nextArgs = [...args];
  const separatorIndex = nextArgs.indexOf("--");
  nextArgs.splice(
    separatorIndex === -1 ? nextArgs.length : separatorIndex,
    0,
    "--config",
    override,
  );
  return nextArgs;
}

async function main() {
  const cli = require("@tauri-apps/cli");
  try {
    await cli.run(withBusinessDockConfig(process.argv.slice(2)), "pnpm tauri");
  } catch (error) {
    cli.logError(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  await main();
}
