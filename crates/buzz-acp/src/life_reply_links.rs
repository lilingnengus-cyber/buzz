use life_workbench_contracts::result::{LifeResourceRef, ResourceType};

// Only enrich existing plain list entries. Never add resources to the answer or
// choose between different actions sharing a title. Existing Markdown is left alone.
pub(super) fn link_action_items(text: &str, refs: &[LifeResourceRef]) -> String {
    let mut fence: Option<char> = None;
    text.lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                let marker = trimmed.chars().next();
                if fence.is_none() {
                    fence = marker;
                } else if fence == marker {
                    fence = None;
                }
                return line.to_owned();
            }
            if fence.is_some() {
                return line.to_owned();
            }
            let Some(item) = trimmed
                .strip_prefix("- ")
                .or_else(|| trimmed.strip_prefix("* "))
            else {
                return line.to_owned();
            };
            // A raw Markdown delimiter can change the structure of the label.
            let matches = refs
                .iter()
                .filter(|reference| {
                    reference.resource_type() == ResourceType::Action
                        && reference.title().is_some_and(|title| {
                            !title.is_empty()
                                && !title.contains(['[', ']', '*', '`', '\\', '<', '>'])
                                && item.strip_prefix(title).is_some_and(|rest| {
                                    rest.is_empty() || rest.starts_with(['（', '('])
                                })
                        })
                })
                .collect::<Vec<_>>();
            let Some(reference) = matches.first() else {
                return line.to_owned();
            };
            if matches
                .iter()
                .any(|other| other.life_uri() != reference.life_uri())
            {
                return line.to_owned();
            }
            let Some(title) = reference.title() else {
                return line.to_owned();
            };
            let prefix = &line[..line.len() - item.len()];
            format!(
                "{prefix}[{title}]({}){}",
                reference.life_uri(),
                &item[title.len()..]
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn action(id: &str, title: &str) -> LifeResourceRef {
        serde_json::from_value(
            json!({"scheme":"life","type":"action","id":id,"version":1,"title":title}),
        )
        .expect("reference")
    }

    #[test]
    fn links_only_listed_actions_and_preserves_status() {
        let refs = [
            action("child-1", "公司核名"),
            action("unrelated", "无关行动"),
        ];
        assert_eq!(
            link_action_items(
                "- 公司核名（已完成）\n- 未知行动（待办）\n已完成 1/2",
                &refs
            ),
            "- [公司核名](life://action/child-1)（已完成）\n- 未知行动（待办）\n已完成 1/2"
        );
    }

    #[test]
    fn ambiguous_titles_and_markdown_are_not_rewritten() {
        let refs = [action("child-1", "同名"), action("child-2", "同名")];
        assert_eq!(link_action_items("- 同名（待办）", &refs), "- 同名（待办）");
        let refs = [action("child-1", "公司核名")];
        for text in [
            "- [公司核名](life://action/child-1)（待办）",
            "```\n- 公司核名（待办）\n```",
            "- 公司核名附件（待办）",
        ] {
            assert_eq!(link_action_items(text, &refs), text);
        }
    }
}
