//! Keep each session within the native 128-tool limit without widening authority.
use super::*;
pub(super) fn router(config: &Config) -> Result<ToolRouter<BusinessReadMcp>, String> {
    let mut router = BusinessReadMcp::all_tools();
    let approval = if let Some(scope) = &config.approval_scope {
        let kind = scope
            .strip_suffix(":approve")
            .ok_or("Invalid approval visibility scope")?;
        let name = format!("approve_{}", kind.strip_suffix("_intent").unwrap_or(kind));
        if !config.chat_approval_enabled
            || !router
                .list_all()
                .iter()
                .any(|tool| tool.name.as_ref() == name)
        {
            return Err("Unsupported approval visibility scope".into());
        }
        Some(name)
    } else {
        None
    };
    for tool in router.list_all() {
        let name = tool.name.as_ref();
        let visible = match &approval {
            Some(approval) => {
                name == approval
                    || !["approve_", "prepare_", "create_", "update_"]
                        .iter()
                        .any(|prefix| name.starts_with(prefix))
            }
            None => !name.starts_with("approve_"),
        };
        if !visible {
            router.remove_route(name);
        }
    }
    if router.list_all().len() > 128 {
        return Err("Business tool profile exceeds native session capacity".into());
    }
    // Visibility is not authorization: every call still consumes/verifies its signed delegation.
    Ok(router)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ordinary_and_every_signed_profile_fit_and_keep_only_matching_approval() {
        let mut config = crate::tests::production_config(
            Url::parse("https://127.0.0.1:9/").unwrap(),
            Uuid::new_v4(),
        );
        let ordinary = router(&config).unwrap().list_all();
        assert!(ordinary.len() <= 128);
        assert!(ordinary.iter().all(|t| !t.name.starts_with("approve_")));
        assert!(ordinary.iter().any(|t| t.name == "prepare_crm_creation"));
        for tool in BusinessReadMcp::all_tools()
            .list_all()
            .into_iter()
            .filter(|t| t.name.starts_with("approve_"))
        {
            let name = tool.name.strip_prefix("approve_").unwrap();
            // CRM/other intents use the same family-to-tool spelling; direct
            // documents omit the suffix. Both must select only one fixed tool.
            config.approval_scope = Some(format!("{name}:approve"));
            let visible = router(&config).unwrap().list_all();
            assert!(visible.len() <= 128);
            assert_eq!(
                visible
                    .iter()
                    .filter(|t| t.name.starts_with("approve_"))
                    .map(|t| t.name.as_ref())
                    .collect::<Vec<_>>(),
                vec![tool.name.as_ref()]
            );
            assert!(visible.iter().all(|t| !["prepare_", "create_", "update_"]
                .iter()
                .any(|p| t.name.starts_with(p))));
        }
        for family in [
            "operating_report_snapshot",
            "management_report_snapshot",
            "sales_order_hold",
            "sales_order_release_hold",
            "core_master_status",
            "product_master_status",
            "core_master_creation",
            "core_master_update",
            "product_master_creation",
            "product_master_update",
        ] {
            assert!(ordinary
                .iter()
                .any(|t| t.name == format!("prepare_{family}")));
            config.approval_scope = Some(format!("{family}_intent:approve"));
            let selected = router(&config).unwrap().list_all();
            let tool = selected
                .iter()
                .find(|t| t.name == format!("approve_{family}"))
                .unwrap();
            assert!(tool
                .input_schema
                .get("properties")
                .is_none_or(|p| p.as_object().is_some_and(|o| o.is_empty())));
        }
        assert!(ordinary
            .iter()
            .any(|t| t.name == "get_business_master_record"));
        config.approval_scope = Some("crm_followup_intent:approve".into());
        assert!(router(&config)
            .unwrap()
            .list_all()
            .iter()
            .any(|t| t.name == "approve_crm_followup"));
        config.approval_scope = Some("arbitrary:approve".into());
        assert!(router(&config).is_err());
    }
}
