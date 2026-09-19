//! Explicit bounded follow-up content; the generic response guard still forbids notes.
use super::*;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Followup {
    id: Uuid,
    note: String,
    stage: String,
    next_action: String,
    next_follow_up: Option<chrono::NaiveDate>,
    created_at: chrono::DateTime<Utc>,
    author_user_id: Uuid,
}
pub(super) fn validate(
    tool: &str,
    result: BusinessToolResult<Value>,
    context: &DelegationContext,
    max: usize,
) -> Result<BusinessToolResult<Value>, String> {
    if tool != "get_crm_opportunity" {
        return validate_result(result, context, max);
    }
    if result.items.len() != 1 {
        return Err("CRM detail must contain exactly one opportunity".into());
    }
    if serde_json::to_vec(&result)
        .map_err(|_| "Invalid CRM result")?
        .len()
        > max
    {
        return Err("CRM response exceeded payload limit".into());
    }
    let mut checked = result.clone();
    let notes = checked.items[0]
        .as_object_mut()
        .and_then(|v| v.remove("followups"))
        .ok_or("CRM follow-ups missing")?;
    let notes: Vec<Followup> =
        serde_json::from_value(notes).map_err(|_| "Invalid CRM follow-up fields")?;
    if notes.len() > 3 {
        return Err("CRM follow-up page exceeded limit".into());
    }
    for note in notes {
        if note.note.chars().count() > 4000
            || note.next_action.chars().count() > 500
            || !matches!(
                note.stage.as_str(),
                "new" | "contacting" | "quoting" | "won" | "lost"
            )
        {
            return Err("Invalid CRM follow-up content".into());
        }
        // Typed fields are validated during deserialization and deliberately retained.
        let _ = (
            note.id,
            note.next_follow_up,
            note.created_at,
            note.author_user_id,
        );
    }
    validate_result(checked, context, max)?;
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn followup_schema_is_explicit_and_rejects_secrets() {
        let row = json!({"id":Uuid::new_v4(),"note":"Business note","stage":"new","nextAction":"Reply","nextFollowUp":null,"createdAt":Utc::now(),"authorUserId":Uuid::new_v4()});
        assert!(serde_json::from_value::<Followup>(row.clone()).is_ok());
        assert!(validate_business_value(&row).is_err());
        let mut bad = row;
        bad["accessToken"] = "secret".into();
        assert!(serde_json::from_value::<Followup>(bad).is_err());
    }
    #[test]
    fn full_notes_survive_only_the_explicit_crm_detail_validator() {
        let mut context = crate::tests::context();
        context.required_scope = "crm:read".into();
        let note = json!({"id":Uuid::new_v4(),"note":"中".repeat(4000),"stage":"new","nextAction":"Reply","nextFollowUp":null,"createdAt":Utc::now(),"authorUserId":Uuid::new_v4()});
        let result:BusinessToolResult<Value>=serde_json::from_value(json!({"schemaVersion":1,"status":"ok","asOf":Utc::now(),"scopeSummary":{},"summary":{},"items":[{"id":Uuid::new_v4(),"followups":[note.clone(),note.clone(),note.clone()]}],"resourceRefs":[],"evidence":[],"warnings":[],"traceId":context.trace_id})).unwrap();
        let accepted = validate("get_crm_opportunity", result.clone(), &context, 51200).unwrap();
        assert_eq!(accepted.items, result.items);
        assert!(validate("search_crm_opportunities", result.clone(), &context, 51200).is_err());
        assert!(validate("get_crm_opportunity", result.clone(), &context, 1000).is_err());
        let mut bad = result.clone();
        bad.items[0]["followups"][0]["accessToken"] = "secret".into();
        assert!(validate("get_crm_opportunity", bad, &context, 51200).is_err());
        let mut bad = result;
        bad.items[0]["followups"] = json!([note.clone(), note.clone(), note.clone(), note]);
        assert!(validate("get_crm_opportunity", bad, &context, 131072).is_err());
    }
}
