//! Resolve the repository that belongs to a project home channel.
//!
//! Channel-first projects bind a default `kind:30617` at create time. Creating
//! a task in that channel still has to land on *this* project, so the CLI finds
//! (or creates) a `kind:30617` bound to the same `buzz-channel` rather than
//! asking the caller to invent a second project.

use buzz_core::kind::KIND_GIT_REPO_ANNOUNCEMENT;
use nostr::{Event, Tag};

use crate::client::BuzzClient;
use crate::commands::projects::{fetch_projects_for_channel, try_add_own_repo_to_channel_project};
use crate::error::CliError;
use crate::validate::{validate_repo_id, validate_uuid};

pub struct ChannelProjectRepo {
    pub repo_owner: String,
    pub repo_id: String,
}

/// Find this channel's project repository, creating one when the project has none.
pub async fn resolve_or_ensure_repo_for_channel(
    client: &BuzzClient,
    channel: &str,
) -> Result<ChannelProjectRepo, CliError> {
    validate_uuid(channel)?;
    let projects = fetch_projects_for_channel(client, channel).await?;
    let project = pick_oldest_listed(&projects);
    if let Some(event) = project {
        if let Some(member) = first_member_repo(event) {
            return Ok(member);
        }
    }

    if let Some(repo) = pick_channel_repo(client, channel).await? {
        let caller = client.keys().public_key().to_hex();
        if repo.repo_owner.eq_ignore_ascii_case(&caller) {
            let _ = try_add_own_repo_to_channel_project(client, channel, &repo.repo_id).await;
        }
        return Ok(repo);
    }

    let Some(event) = project else {
        return Err(CliError::Usage(
            "this channel is not a project home; pass --repo-owner and --repo-id".into(),
        ));
    };
    ensure_default_repo(client, channel, event).await
}

fn pick_oldest_listed(events: &[Event]) -> Option<&Event> {
    events
        .iter()
        .filter(|event| !project_is_unlisted(event))
        .min_by_key(|event| event.created_at)
}

fn project_is_unlisted(event: &Event) -> bool {
    event.tags.iter().any(|tag| {
        matches!(
            tag.as_slice(),
            [name, value, ..] if name == "buzz-visibility" && value == "unlisted"
        )
    })
}

fn project_dtag(event: &Event) -> Option<String> {
    event.tags.iter().find_map(|tag| match tag.as_slice() {
        [name, value, ..] if name == "d" && !value.is_empty() => Some(value.clone()),
        _ => None,
    })
}

fn project_name(event: &Event) -> Option<String> {
    event.tags.iter().find_map(|tag| match tag.as_slice() {
        [name, value, ..] if name == "name" && !value.is_empty() => Some(value.clone()),
        _ => None,
    })
}

fn first_member_repo(event: &Event) -> Option<ChannelProjectRepo> {
    event.tags.iter().find_map(|tag| match tag.as_slice() {
        [name, value, ..] if name == "a" => parse_repo_a_tag(value),
        _ => None,
    })
}

pub(crate) fn parse_repo_a_tag(value: &str) -> Option<ChannelProjectRepo> {
    let mut parts = value.splitn(3, ':');
    let kind = parts.next()?;
    let owner = parts.next()?.trim();
    let id = parts.next()?.trim();
    if kind != "30617" || owner.len() != 64 || id.is_empty() {
        return None;
    }
    Some(ChannelProjectRepo {
        repo_owner: owner.to_ascii_lowercase(),
        repo_id: id.to_string(),
    })
}

fn repo_is_unlisted(event: &Event) -> bool {
    event.tags.iter().any(|tag| {
        matches!(
            tag.as_slice(),
            [name, value, ..] if name == "buzz-visibility" && value == "unlisted"
        )
    })
}

fn repo_dtag(event: &Event) -> Option<String> {
    event.tags.iter().find_map(|tag| match tag.as_slice() {
        [name, value, ..] if name == "d" && !value.is_empty() => Some(value.clone()),
        _ => None,
    })
}

async fn pick_channel_repo(
    client: &BuzzClient,
    channel: &str,
) -> Result<Option<ChannelProjectRepo>, CliError> {
    let filter = serde_json::json!({
        "kinds": [KIND_GIT_REPO_ANNOUNCEMENT],
        "#buzz-channel": [channel],
        "limit": 20,
    });
    let raw = client.query(&filter).await?;
    let events: Vec<Event> = serde_json::from_str(&raw)
        .map_err(|error| CliError::Other(format!("failed to parse relay response: {error}")))?;
    let Some(event) = events
        .iter()
        .filter(|event| !repo_is_unlisted(event))
        .min_by_key(|event| event.created_at)
    else {
        return Ok(None);
    };
    let Some(repo_id) = repo_dtag(event) else {
        return Ok(None);
    };
    Ok(Some(ChannelProjectRepo {
        repo_owner: event.pubkey.to_hex(),
        repo_id,
    }))
}

async fn ensure_default_repo(
    client: &BuzzClient,
    channel: &str,
    project: &Event,
) -> Result<ChannelProjectRepo, CliError> {
    let slug = project_dtag(project)
        .ok_or_else(|| CliError::Other("project announcement is missing its d tag".into()))?;
    let repo_id = repo_id_from_project_slug(&slug)?;
    let name = project_name(project).unwrap_or_else(|| slug.clone());
    let name = truncate_repo_name(&name);
    let caller = client.keys().public_key().to_hex();
    let project_owner = project.pubkey.to_hex();

    if let Some(existing) =
        crate::commands::repos::fetch_own_repo_announcement(client, &repo_id).await?
    {
        let _ = try_add_own_repo_to_channel_project(client, channel, &repo_id).await;
        return Ok(ChannelProjectRepo {
            repo_owner: existing.pubkey.to_hex(),
            repo_id,
        });
    }

    let mut builder = crate::commands::repos::build_create_announcement(
        &repo_id,
        Some(&name),
        None,
        &[],
        None,
        &[],
        Some(channel),
    )?;
    if !project_owner.eq_ignore_ascii_case(&caller) {
        builder = builder.tag(Tag::parse(["maintainers", project_owner.as_str()]).map_err(
            |error| {
                CliError::Other(format!(
                    "failed to tag project owner as maintainer: {error}"
                ))
            },
        )?);
    }
    let event = client.sign_event(builder)?;
    client.submit_event(event).await?;
    let _ = try_add_own_repo_to_channel_project(client, channel, &repo_id).await;
    Ok(ChannelProjectRepo {
        repo_owner: caller,
        repo_id,
    })
}

pub(crate) fn repo_id_from_project_slug(slug: &str) -> Result<String, CliError> {
    if validate_repo_id(slug).is_ok() {
        return Ok(slug.to_string());
    }
    let mut out = String::new();
    for ch in slug.chars() {
        if out.len() >= 64 {
            break;
        }
        if ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' || ch == '-' {
            out.push(ch);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    while out.starts_with('.') {
        out.remove(0);
    }
    if out.ends_with('-') {
        out.pop();
    }
    validate_repo_id(&out)?;
    Ok(out)
}

fn truncate_repo_name(name: &str) -> String {
    if name.len() <= 128 {
        return name.to_string();
    }
    name.chars().take(128).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_id_from_project_slug_keeps_valid_ids() {
        assert_eq!(
            repo_id_from_project_slug("space-invaders-3d").unwrap(),
            "space-invaders-3d"
        );
    }

    #[test]
    fn repo_id_from_project_slug_sanitizes_invalid_characters() {
        assert_eq!(
            repo_id_from_project_slug("Space Invaders 3D!").unwrap(),
            "Space-Invaders-3D"
        );
    }

    #[test]
    fn parse_repo_a_tag_reads_nip34_coordinate() {
        let owner = "a".repeat(64);
        let parsed = parse_repo_a_tag(&format!("30617:{owner}:game")).unwrap();
        assert_eq!(parsed.repo_owner, owner);
        assert_eq!(parsed.repo_id, "game");
    }
}
