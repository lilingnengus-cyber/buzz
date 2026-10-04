use super::*;
use unicode_normalization::UnicodeNormalization;

fn normalized(name: &str) -> String {
    name.nfkc()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}
fn matches(left: &str, right: &str) -> bool {
    let a = normalized(left);
    let b = normalized(right);
    !a.is_empty()
        && (a == b
            || (a.chars().count().min(b.chars().count()) >= 4
                && (a.contains(&b) || b.contains(&a))))
}

pub(super) async fn check(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    actor: Uuid,
    name: &str,
) -> Result<(), DomainError> {
    let rows: Vec<(String, String, String)> = sqlx::query_as("SELECT c.code,c.name,c.status FROM business_customers c JOIN business_customer_scopes s ON s.customer_id=c.id WHERE s.enterprise_user_id=$1 ORDER BY c.code,c.id")
        .bind(actor).fetch_all(&mut **tx).await?;
    let duplicates: Vec<_> = rows
        .iter()
        .filter(|(_, existing, _)| matches(name, existing))
        .collect();
    if duplicates.is_empty() {
        return Ok(());
    }
    let examples = duplicates
        .iter()
        .take(5)
        .map(|(code, name, status)| {
            format!(
                "{code} · {name}{}",
                if status == "disabled" {
                    " · 已停用"
                } else {
                    ""
                }
            )
        })
        .collect::<Vec<_>>()
        .join("；");
    Err(DomainError::Invalid(format!("DUPLICATE_CUSTOMER:发现 {} 条可能重复客户：{examples}。尚未创建。请核对后明确确认是否仍新建独立客户。", duplicates.len())))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_follow_the_page_matching_policy() {
        assert!(matches("ＡＢＣ 公司", "abc公司"));
        assert!(matches("杭州测试公司", "杭州测试公司分部"));
        assert!(!matches("公司", "另一家公司"));
        assert!(!matches("", ""));
        assert!(!matches("甲方有限公司", "乙方有限公司"));
    }
}
