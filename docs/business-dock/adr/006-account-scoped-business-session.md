# ADR 006: Account-scoped Business session

- Status: Accepted
- Date: 2026-08-28

## Context

The original Desktop flow required two proofs after Authentik login: a valid
Workbench OIDC session and a signed Buzz identity-binding challenge. Device
metadata was later removed from authorization, but the remaining public-key
binding still blocked account login when the callback or local signer state did
not match.

## Decision

Interactive Business Dock authorization is scoped to the authenticated
enterprise account. A valid Authentik Workbench access token creates or resumes
the server-side Workbench session and may request a one-time Embed Session
without a device identifier, Buzz public key, or local signature.

Embed and Business sessions retain an optional legacy binding reference for
historical rows, but newly issued interactive sessions store `NULL`. Existing
identity-binding endpoints and tables remain available for compatibility and
agent-delegation flows; they are not part of interactive login.

The remaining controls are unchanged: exact OIDC issuer/client/audience
validation, active Workbench session checks, target-path validation, a
30-second single-use hashed ticket, deployment/audience binding, issuance rate
limits, host-only HttpOnly cookies, CSRF checks, logout cascades, and append-only
audit.

## Consequences

An enterprise account may open Business Dock from any supported Workbench
device after completing Authentik login. Revoking a Buzz key no longer revokes
account-scoped Business sessions; Workbench logout, Authentik back-channel
logout, user disablement, expiry, or explicit Business logout still does.

This intentionally makes Authentik the sole interactive account boundary and
trades device/key possession enforcement for a reliable account-login flow.
