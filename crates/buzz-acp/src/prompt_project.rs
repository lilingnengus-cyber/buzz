//! Parse a channel's NIP-MP project home for ACP `[Context]`.

use serde_json::Value;

/// Project identity attached to a home channel in agent prompts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PromptProjectInfo {
    pub name: String,
    pub slug: String,
    pub owner: String,
    pub coordinate: String,
    pub default_repo_owner: Option<String>,
    pub default_repo_id: Option<String>,
}

/// Pick the oldest listed `kind:30621` bound to the channel.
///
/// Unlisted heads are ignored. When two listed projects share the channel
/// (a duplicate card), the earlier head is the original home.
pub fn pick_listed_project_home(events: &[Value], channel_id: &str) -> Option<PromptProjectInfo> {
    let mut listed: Vec<&Value> = events
        .iter()
        .filter(|event| {
            !event_is_unlisted(event) && event_has_tag_value(event, "buzz-channel", channel_id)
        })
        .collect();
    listed.sort_by_key(|event| {
        event
            .get("created_at")
            .and_then(Value::as_u64)
            .unwrap_or(u64::MAX)
    });
    listed.into_iter().find_map(parse_prompt_project)
}

fn event_has_tag_value(event: &Value, name: &str, value: &str) -> bool {
    event
        .get("tags")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|tag| {
            let values = tag.as_array();
            values
                .and_then(|parts| parts.first())
                .and_then(Value::as_str)
                == Some(name)
                && values
                    .and_then(|parts| parts.get(1))
                    .and_then(Value::as_str)
                    == Some(value)
        })
}

fn event_is_unlisted(event: &Value) -> bool {
    event
        .get("tags")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|tag| {
            let arr = tag.as_array();
            matches!(
                arr.map(|values| {
                    values.first().and_then(Value::as_str) == Some("buzz-visibility")
                        && values.get(1).and_then(Value::as_str) == Some("unlisted")
                }),
                Some(true)
            )
        })
}

fn parse_prompt_project(event: &Value) -> Option<PromptProjectInfo> {
    let owner = event
        .get("pubkey")
        .and_then(Value::as_str)?
        .trim()
        .to_ascii_lowercase();
    if owner.len() != 64 {
        return None;
    }
    let tags = event.get("tags").and_then(Value::as_array)?;
    let mut slug = None;
    let mut name = None;
    let mut default_repo = None;
    for tag in tags {
        let Some(arr) = tag.as_array() else { continue };
        match arr.first().and_then(Value::as_str) {
            Some("d") if slug.is_none() => {
                slug = arr
                    .get(1)
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string);
            }
            Some("name") if name.is_none() => {
                name = arr
                    .get(1)
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string);
            }
            Some("a") if default_repo.is_none() => {
                default_repo = arr
                    .get(1)
                    .and_then(Value::as_str)
                    .and_then(parse_repo_coord);
            }
            _ => {}
        }
    }
    let slug = slug?;
    let owner_for_coord = owner.clone();
    Some(PromptProjectInfo {
        name: name.unwrap_or_else(|| slug.clone()),
        coordinate: format!("30621:{owner_for_coord}:{slug}"),
        slug,
        owner,
        default_repo_owner: default_repo
            .as_ref()
            .map(|(repo_owner, _)| repo_owner.clone()),
        default_repo_id: default_repo.map(|(_, repo_id)| repo_id),
    })
}

fn parse_repo_coord(value: &str) -> Option<(String, String)> {
    let mut parts = value.splitn(3, ':');
    let kind = parts.next()?;
    let owner = parts.next()?.trim().to_ascii_lowercase();
    let id = parts.next()?.trim();
    if kind != "30617" || owner.len() != 64 || id.is_empty() {
        return None;
    }
    Some((owner, id.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const CHANNEL_ID: &str = "11111111-1111-4111-8111-111111111111";

    fn project_event(
        created_at: u64,
        owner: &str,
        slug: &str,
        name: &str,
        extra_tags: Vec<Value>,
    ) -> Value {
        let mut tags = vec![
            json!(["d", slug]),
            json!(["name", name]),
            json!(["buzz-channel", CHANNEL_ID]),
        ];
        tags.extend(extra_tags);
        json!({
            "pubkey": owner,
            "created_at": created_at,
            "kind": 30621,
            "tags": tags,
        })
    }

    #[test]
    fn pick_listed_project_home_prefers_oldest_listed() {
        let owner = "a".repeat(64);
        let later = "b".repeat(64);
        let events = vec![
            project_event(
                200,
                &later,
                "space-invaders-3d",
                "Agent copy",
                vec![json!(["a", format!("30617:{later}:space-invaders-3d")])],
            ),
            project_event(
                100,
                &owner,
                "space-invaders-3d",
                "Space Invaders 3D",
                vec![],
            ),
        ];
        let home = pick_listed_project_home(&events, CHANNEL_ID).expect("listed project");
        assert_eq!(home.owner, owner);
        assert_eq!(home.name, "Space Invaders 3D");
        assert_eq!(home.slug, "space-invaders-3d");
        assert_eq!(home.coordinate, format!("30621:{owner}:space-invaders-3d"));
        assert_eq!(home.default_repo_id, None);
    }

    #[test]
    fn pick_listed_project_home_skips_unlisted_and_reads_member_repo() {
        let owner = "a".repeat(64);
        let repo_owner = "c".repeat(64);
        let events = vec![
            project_event(
                50,
                &owner,
                "hidden",
                "Hidden",
                vec![json!(["buzz-visibility", "unlisted"])],
            ),
            project_event(
                80,
                &owner,
                "space-invaders-3d",
                "Space Invaders 3D",
                vec![json!(["a", format!("30617:{repo_owner}:game")])],
            ),
        ];
        let home = pick_listed_project_home(&events, CHANNEL_ID).expect("listed project");
        assert_eq!(home.slug, "space-invaders-3d");
        assert_eq!(
            home.default_repo_owner.as_deref(),
            Some(repo_owner.as_str())
        );
        assert_eq!(home.default_repo_id.as_deref(), Some("game"));
    }

    #[test]
    fn pick_listed_project_home_ignores_other_channels() {
        let owner = "a".repeat(64);
        let mut unrelated = project_event(1, &owner, "wrong", "Wrong", vec![]);
        unrelated["tags"][2][1] = json!("22222222-2222-4222-8222-222222222222");
        let expected = project_event(2, &owner, "right", "Right", vec![]);

        let home =
            pick_listed_project_home(&[unrelated, expected], CHANNEL_ID).expect("project home");
        assert_eq!(home.slug, "right");
    }
}
