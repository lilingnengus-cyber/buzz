/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_BUSINESS_IAM_ADMIN_URL?: string;
  readonly VITE_BUSINESS_AUTH_GATEWAY_URL?: string;
  readonly VITE_OIDC_ISSUER?: string;
  readonly VITE_OIDC_CLIENT_ID?: string;
  readonly VITE_OIDC_REDIRECT_URI?: string;
  readonly VITE_OIDC_POST_LOGOUT_REDIRECT_URI?: string;
  readonly VITE_OIDC_DESKTOP_PROXY_ORIGIN?: string;
  readonly VITE_BUSINESS_OIDC_ISSUER?: string;
  readonly VITE_BUSINESS_OIDC_CLIENT_ID?: string;
  readonly VITE_BUSINESS_OIDC_REDIRECT_URI?: string;
  readonly VITE_BUSINESS_APP_ORIGIN?: string;
  readonly VITE_BUSINESS_APP_URL?: string;
  readonly VITE_LIFE_OIDC_ISSUER?: string;
  readonly VITE_LIFE_OIDC_CLIENT_ID?: string;
  readonly VITE_LIFE_OIDC_REDIRECT_URI?: string;
  readonly VITE_LIFE_OIDC_POST_LOGOUT_REDIRECT_URI?: string;
  readonly VITE_LIFE_OIDC_DESKTOP_PROXY_ORIGIN?: string;
  readonly VITE_LIFE_DOCK_ENABLED?: string;
  readonly VITE_LIFE_AUTH_GATEWAY_URL?: string;
  readonly VITE_LIFE_APP_ORIGIN?: string;
  readonly VITE_LIFE_APP_URL?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}

interface Window {
  __BUZZ_E2E_WORKBENCH_ACCESS_TOKEN__?: string;
  __BUZZ_E2E_LIFE_ACCESS_TOKEN__?: string;
}
