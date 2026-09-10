import { lifeSessionRenewalDelay } from "./lifeEmbedSession";

/** Keep an authenticated session alive independently of panel visibility. */
export function watchLifeSessionRenewal(
  expiresAt: string,
  renew: () => void,
  host: Pick<
    Window,
    | "setTimeout"
    | "clearTimeout"
    | "addEventListener"
    | "removeEventListener"
    | "document"
  > = window,
): () => void {
  let attempted = false;
  const check = () => {
    if (attempted || lifeSessionRenewalDelay(expiresAt) !== 0) return;
    attempted = true;
    renew();
  };
  const visible = () => {
    if (host.document.visibilityState === "visible") check();
  };
  const delay = lifeSessionRenewalDelay(expiresAt);
  if (delay === null) return () => {};
  const timer = host.setTimeout(check, delay);
  host.addEventListener("focus", check);
  host.addEventListener("online", check);
  host.document.addEventListener("visibilitychange", visible);
  return () => {
    attempted = true;
    host.clearTimeout(timer);
    host.removeEventListener("focus", check);
    host.removeEventListener("online", check);
    host.document.removeEventListener("visibilitychange", visible);
  };
}
