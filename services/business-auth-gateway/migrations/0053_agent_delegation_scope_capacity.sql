-- Additional fixed business capabilities must not displace earlier requested scopes.
-- This raises only the storage bound; issuance still checks the fixed allowlist,
-- live human/agent IAM grants, feature switches and exact signed approval scope.
ALTER TABLE agent_read_delegations
 DROP CONSTRAINT agent_read_delegations_scopes_check,
 ADD CONSTRAINT agent_read_delegations_scopes_check CHECK(cardinality(scopes) BETWEEN 1 AND 128);
