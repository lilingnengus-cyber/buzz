//! Capability-specific adaptation of restricted IAM grants to B1 scope sets.
use super::{stable_hash, PgStore, StoreError};
use crate::model::{AuthorizationSnapshot, DataScopes};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

const DIMENSIONS: [&str; 6] = [
    "legal_entity",
    "warehouse",
    "customer",
    "supplier",
    "brand",
    "business_unit",
];
type Rectangle = [BTreeSet<Uuid>; 6];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RestrictedScope {
    mode: String,
    dimensions: BTreeMap<String, BTreeSet<Uuid>>,
}

impl PgStore {
    /// Resolves a capability's own IAM scope without borrowing another capability's grants.
    /// Legacy grants retain their existing scope semantics. Non-rectangular unions fail
    /// closed because the existing domain queries consume six independent scope sets.
    pub async fn snapshot_for_permission(
        &self,
        user: Uuid,
        permission: &str,
    ) -> Result<AuthorizationSnapshot, StoreError> {
        let mut snapshot = self.snapshot(user).await?;
        // Only surfaces whose queries enforce all relevant scope dimensions are
        // adapted here. Global administration and financial commands keep their
        // existing authorization path until separately validated.
        if !matches!(
            permission,
            "crm:read" | "crm:manage" | "business_master_data:read"
        ) {
            return Ok(snapshot);
        }
        let rows: Vec<(String, Value)> = sqlx::query_as(
            "SELECT permission.capability, grant_row.data_scope
             FROM business_iam.principals principal
             JOIN (
               SELECT principal_id,permission_id,data_scope,obligations
               FROM business_iam.principal_permissions
               WHERE valid_from<=now() AND (valid_until IS NULL OR valid_until>now())
               UNION ALL
               SELECT assignment.principal_id,rp.permission_id,rp.data_scope,rp.obligations
               FROM business_iam.principal_roles assignment
               JOIN business_iam.roles role ON role.id=assignment.role_id AND role.status='active'
               JOIN business_iam.role_permissions rp ON rp.role_id=role.id
               WHERE assignment.valid_from<=now()
                 AND (assignment.valid_until IS NULL OR assignment.valid_until>now())
             ) grant_row ON grant_row.principal_id=principal.id
             JOIN business_iam.permissions permission
               ON permission.id=grant_row.permission_id AND permission.status='active'
             WHERE principal.kind='human' AND principal.status='active'
               AND principal.external_id=$1 AND grant_row.data_scope->>'mode'='restricted'
               AND permission.capability IN ('crm:read','crm:manage','business_master_data:read')
               AND grant_row.obligations='[]'::jsonb AND permission.obligations='[]'::jsonb",
        )
        .bind(user.to_string())
        .fetch_all(&self.pool)
        .await?;
        if !rows.iter().any(|(capability, _)| capability == permission) {
            return Ok(snapshot);
        }
        // Omitted dimensions are unrestricted, not empty. Resolve them against current
        // master data on every request; explicit operating units are never expanded.
        let resources: Vec<(String, Uuid)> = sqlx::query_as(
            "SELECT resource_type,id FROM business_master_data_directory
             WHERE resource_type=ANY($1)",
        )
        .bind(DIMENSIONS.to_vec())
        .fetch_all(&self.pool)
        .await?;
        let mut universe: Rectangle = Default::default();
        for (kind, id) in resources {
            if let Some(index) = DIMENSIONS.iter().position(|dimension| *dimension == kind) {
                universe[index].insert(id);
            }
        }
        let legacy = rectangle(&snapshot.scopes);
        let mut grants: BTreeMap<String, Vec<Rectangle>> = BTreeMap::new();
        for capability in &snapshot.permission_keys {
            grants
                .entry(capability.clone())
                .or_default()
                .push(legacy.clone());
        }
        for (capability, raw) in rows {
            if let Some(scope) = resolve_scope(raw, &universe) {
                grants.entry(capability).or_default().push(scope);
            }
        }
        let scopes = grants
            .get(permission)
            .and_then(|scopes| exact_union(scopes.clone()))
            .ok_or(StoreError::NotFoundOrForbidden)?;
        // UI hints such as canManage must also be valid throughout this snapshot's
        // scope. Actual writes still request their own capability-specific snapshot.
        snapshot.permission_keys = grants
            .into_iter()
            .filter_map(|(capability, grants)| {
                exact_union(grants)
                    .filter(|grant| contains(grant, &scopes))
                    .map(|_| capability)
            })
            .collect();
        snapshot.scopes = data_scopes(scopes);
        snapshot.effective_scope_hash = stable_hash(&(
            user,
            snapshot.scope_version,
            &snapshot.roles,
            &snapshot.permission_keys,
            &snapshot.scopes,
        ))?;
        Ok(snapshot)
    }
}

fn resolve_scope(raw: Value, universe: &Rectangle) -> Option<Rectangle> {
    let scope: RestrictedScope = serde_json::from_value(raw).ok()?;
    if scope.mode != "restricted" || scope.dimensions.is_empty() {
        return None;
    }
    // This bridge currently supports operating-unit-only grants. CRM permits
    // unconverted records without a customer and independent master records
    // have no organization association, so other dimensions need resource-aware
    // predicates before they can be safely consumed. Never ignore them.
    if scope
        .dimensions
        .keys()
        .any(|dimension| dimension != "business_unit")
    {
        return None;
    }
    let mut result = universe.clone();
    for (dimension, ids) in scope.dimensions {
        let index = DIMENSIONS.iter().position(|name| *name == dimension)?;
        if ids.is_empty() || !ids.is_subset(&universe[index]) {
            return None;
        }
        result[index] = ids;
    }
    Some(result)
}

fn rectangle(scopes: &DataScopes) -> Rectangle {
    [
        scopes.legal_entity_ids.clone(),
        scopes.warehouse_ids.clone(),
        scopes.customer_ids.clone(),
        scopes.supplier_ids.clone(),
        scopes.brand_ids.clone(),
        scopes.business_unit_ids.clone(),
    ]
}
fn data_scopes(
    [legal_entity_ids, warehouse_ids, customer_ids, supplier_ids, brand_ids, business_unit_ids]: Rectangle,
) -> DataScopes {
    DataScopes {
        legal_entity_ids,
        warehouse_ids,
        customer_ids,
        supplier_ids,
        brand_ids,
        business_unit_ids,
    }
}
fn contains(left: &Rectangle, right: &Rectangle) -> bool {
    left.iter()
        .zip(right)
        .all(|(left, right)| right.is_subset(left))
}

// Merge only containment or one differing dimension. Unioning every dimension
// independently would invent unauthorized pairs, e.g. (legal A, unit B).
fn exact_union(mut scopes: Vec<Rectangle>) -> Option<Rectangle> {
    loop {
        let mut pair = None;
        'outer: for i in 0..scopes.len() {
            for j in i + 1..scopes.len() {
                if contains(&scopes[i], &scopes[j])
                    || contains(&scopes[j], &scopes[i])
                    || scopes[i]
                        .iter()
                        .zip(&scopes[j])
                        .filter(|(a, b)| a != b)
                        .count()
                        <= 1
                {
                    pair = Some((i, j));
                    break 'outer;
                }
            }
        }
        let Some((i, j)) = pair else { break };
        let other = scopes.remove(j);
        for (dimension, ids) in scopes[i].iter_mut().zip(other) {
            dimension.extend(ids);
        }
    }
    (scopes.len() == 1).then(|| scopes.remove(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn scope_uses_exact_units_and_rejects_unknown_dimensions_or_ids() {
        let a = Uuid::from_u128(1);
        let b = Uuid::from_u128(2);
        let all: Rectangle = std::array::from_fn(|_| BTreeSet::from([a, b]));
        let resolved = resolve_scope(
            json!({"mode":"restricted","dimensions":{"business_unit":[a]}}),
            &all,
        )
        .unwrap();
        assert_eq!(resolved[5], BTreeSet::from([a]));
        assert_eq!(resolved[2], all[2]);
        for dimensions in [
            json!({"business_unit":[]}),
            json!({"unknown":[a]}),
            json!({"customer":[a]}),
            json!({"legal_entity":[a],"business_unit":[a]}),
            json!({"business_unit":[Uuid::nil()]}),
            json!({}),
        ] {
            assert!(
                resolve_scope(json!({"mode":"restricted","dimensions":dimensions}), &all).is_none()
            );
        }
    }
    #[test]
    fn union_never_invents_cross_dimension_pairs() {
        let a = Uuid::from_u128(1);
        let b = Uuid::from_u128(2);
        let first: Rectangle = std::array::from_fn(|_| BTreeSet::from([a]));
        let mut second = first.clone();
        second[5] = BTreeSet::from([b]);
        assert_eq!(
            exact_union(vec![first.clone(), second.clone()]).unwrap()[5],
            BTreeSet::from([a, b])
        );
        second[0] = BTreeSet::from([b]);
        assert!(exact_union(vec![first, second]).is_none());
    }
}
