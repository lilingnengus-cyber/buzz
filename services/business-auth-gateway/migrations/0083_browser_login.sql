-- Short-lived, browser-bound, one-use OIDC authorization transactions.
CREATE TABLE browser_login_transactions (
    state_hash bytea PRIMARY KEY,
    browser_hash bytea NOT NULL,
    verifier text NOT NULL,
    nonce text NOT NULL,
    return_to text NOT NULL,
    expires_at timestamptz NOT NULL DEFAULT now() + interval '5 minutes'
);
CREATE INDEX browser_login_transactions_expiry ON browser_login_transactions(expires_at);
