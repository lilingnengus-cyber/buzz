export type BusinessIamAdminConfig = {
  baseUrl: string;
};

export type BusinessIamAdminEnv = {
  VITE_BUSINESS_IAM_ADMIN_URL?: string;
};

export type BusinessIamAdminConfigResult =
  | { config: BusinessIamAdminConfig; error: null }
  | { config: null; error: string | null };

export function readBusinessIamAdminConfig(
  env: BusinessIamAdminEnv,
): BusinessIamAdminConfigResult {
  const value = env.VITE_BUSINESS_IAM_ADMIN_URL?.trim();
  if (!value) return { config: null, error: null };
  try {
    const url = new URL(value);
    const loopback = ["127.0.0.1", "localhost", "[::1]"].includes(url.hostname);
    if (
      url.username ||
      url.password ||
      url.pathname !== "/" ||
      url.search ||
      url.hash ||
      (url.protocol !== "https:" && !loopback)
    ) {
      return {
        config: null,
        error:
          "Business IAM Admin URL must be an HTTPS origin (loopback HTTP is allowed in development).",
      };
    }
    return { config: { baseUrl: url.origin }, error: null };
  } catch {
    return { config: null, error: "Business IAM Admin URL is invalid." };
  }
}

export function getBusinessIamAdminConfig(): BusinessIamAdminConfigResult {
  return readBusinessIamAdminConfig({
    VITE_BUSINESS_IAM_ADMIN_URL: import.meta.env.VITE_BUSINESS_IAM_ADMIN_URL,
  });
}
