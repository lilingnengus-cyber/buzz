# LifeOS browser login return page

The old OIDC redirect jumps directly from authentik to a custom desktop scheme,
leaving the authentication page displaying its loading stage after application
authentication succeeds. The LifeOS public /auth/pacioli route now renders a
clear return-to-app page before handing code/state to the fixed
pacioli://auth/life-callback URI. It provides a manual open-app link, distinct
error/invalid states and no perpetual spinner. The wording does not claim the
desktop token exchange succeeded without observing it.

The page removes query parameters from browser history, uses no-store and
no-referrer, loads no third-party assets, and applies a nonce CSP. State/code
presence, duplicates and basic bounds are checked; the desktop OIDC manager
continues to validate state/PKCE and redeem the authorization code. The page does
not perform token exchange or store credentials.

The active desktop Life config uses the exact HTTPS redirect. Its Life-only
callback adapter maps the fixed custom scheme handoff back to that registered
redirect for the existing OIDC callback handler. Other callbacks and old clients
are unchanged. The authentik Life provider retains old exact redirect URIs and
adds only https://life.shiyueshizi.com/auth/pacioli.

Local browser tests cover valid/error/invalid returns, fixed handoff target,
query cleanup and response headers. Three desktop config/callback tests and
TypeScript checks passed. Deployment initially stopped because the production
checkout had an unpushed timezone fix (6cf162d); preserve it by merging into main.
