import { createRequire } from "node:module";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { loadEnv } from "vite";

const require = createRequire(import.meta.url);

export function normalizeBusinessOrigin(value) {
  if (!value?.trim()) return null;
  const url = new URL(value.trim());
  if (
    !["http:", "https:"].includes(url.protocol) ||
    url.username ||
    url.password ||
    url.pathname !== "/" ||
    url.search ||
    url.hash
  ) {
    throw new Error(
      "VITE_BUSINESS_APP_ORIGIN must be an HTTP(S) origin without credentials, path, query, or fragment",
    );
  }
  return url.origin;
}

export function buildBusinessDockCsp(baseCsp, configuredOrigin) {
  const origin = normalizeBusinessOrigin(configuredOrigin);
  const directives = baseCsp
    .split(";")
    .map((directive) => directive.trim())
    .filter(Boolean)
    .filter((directive) => !directive.startsWith("frame-src "));
  directives.push(`frame-src 'self'${origin ? ` ${origin}` : ""}`);
  return directives.join("; ");
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
  const viteEnv = loadEnv(mode, process.cwd(), "VITE_");
  const csp = buildBusinessDockCsp(
    tauriConfig.app.security.csp,
    process.env.VITE_BUSINESS_APP_ORIGIN ?? viteEnv.VITE_BUSINESS_APP_ORIGIN,
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
