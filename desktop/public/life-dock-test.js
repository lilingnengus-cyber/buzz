const hostOrigin = window.location.origin;
const bridge = document.querySelector("#bridge");
const theme = document.querySelector("#theme");
const currentResource = document.querySelector("#current-resource");
const refreshCount = document.querySelector("#refresh-count");
const bootstrap = document.querySelector("#bootstrap");
const instance = document.querySelector("#instance");
let nonce = null;
let requestSequence = 0;
let refreshes = 0;
let resource = {
  version: 1,
  extensionId: "life",
  type: "dashboard",
  path: "/embed/dashboard",
};
const instanceId = crypto.randomUUID();
const bootstrapped = new URLSearchParams(window.location.search).has(
  "bootstrap",
);
instance.textContent = instanceId;
bootstrap.textContent = bootstrapped ? "redeemed" : "direct";

function send(version, type, payload) {
  if (!nonce) return;
  requestSequence += 1;
  window.parent.postMessage(
    {
      version,
      type,
      requestId: `life-fixture-${requestSequence}`,
      sessionNonce: nonce,
      ...(payload === undefined ? {} : { payload }),
    },
    hostOrigin,
  );
}

function parseResource(path) {
  if (path === "/embed/dashboard")
    return { version: 1, extensionId: "life", type: "dashboard", path };
  const match = path.match(
    /^\/embed\/(actions|goals|projects|knowledge|journal|reviews|ai-executions|drafts|domains)\/([A-Za-z0-9._~-]+)$/,
  );
  if (!match) return null;
  const types = {
    actions: "action",
    goals: "goal",
    projects: "project",
    knowledge: "knowledge",
    journal: "journal",
    reviews: "review",
    "ai-executions": "ai_execution",
    drafts: "draft",
    domains: "domain",
  };
  return {
    version: 1,
    extensionId: "life",
    type: types[match[1]],
    id: match[2],
    path,
  };
}

function announce(next) {
  resource = next;
  currentResource.textContent = `${next.type}${next.id ? ` · ${next.id}` : ""}`;
  send(2, "RESOURCE_CHANGED", { resource: next });
  send(2, "ROUTE_CHANGED", { url: next.path });
  send(2, "TITLE_CHANGED", { title: currentResource.textContent });
}

window.addEventListener("message", (event) => {
  if (event.origin !== hostOrigin || event.source !== window.parent) return;
  const message = event.data;
  if (!message || typeof message.sessionNonce !== "string") return;
  if (message.type === "HOST_INIT" && message.version === 2 && !nonce) {
    nonce = message.sessionNonce;
    window.__LIFE_FIXTURE_NONCE__ = nonce;
    bridge.textContent = "connected-v2";
    send(2, "LIFE_READY");
    send(2, "RESOURCE_CHANGED", { resource });
    return;
  }
  if (message.sessionNonce !== nonce) return;
  if (message.type === "SET_THEME" && message.version === 2) {
    theme.textContent = message.payload?.theme ?? "unknown";
  } else if (message.type === "REFRESH" && message.version === 2) {
    refreshes += 1;
    refreshCount.textContent = String(refreshes);
  } else if (message.type === "NAVIGATE" && message.version === 2) {
    const next = parseResource(message.payload?.path);
    if (next) announce(next);
  } else if (
    message.type === "REQUEST_CURRENT_RESOURCE" &&
    message.version === 2
  ) {
    send(2, "RESOURCE_CHANGED", { resource });
  } else if (message.type === "CHECK_AUTH" && message.version === 3) {
    if (bootstrapped)
      send(3, "AUTH_STATUS", {
        authenticated: true,
        user: { displayName: "Life Fixture User" },
      });
    else send(3, "AUTH_REQUIRED", { reason: "Bootstrap required" });
  } else if (message.type === "LOGOUT" && message.version === 3) {
    send(3, "AUTH_REQUIRED", { reason: "Signed out" });
  }
});

document.querySelector("#open-action").addEventListener("click", () => {
  announce({
    version: 1,
    extensionId: "life",
    type: "action",
    id: "fixture-action",
    path: "/embed/actions/fixture-action",
  });
});
document.querySelector("#mark-dirty").addEventListener("click", () => {
  send(2, "DIRTY_STATE_CHANGED", { dirty: true });
});
document.querySelector("#mark-clean").addEventListener("click", () => {
  send(2, "DIRTY_STATE_CHANGED", { dirty: false });
});
document.querySelector("#expire-session").addEventListener("click", () => {
  send(3, "SESSION_EXPIRED", { reason: "Fixture expiry" });
});
