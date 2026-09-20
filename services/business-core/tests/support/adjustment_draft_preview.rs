use super::*;
pub async fn verify(
    pool: &PgPool,
    service: &AdjustmentService,
    f: &Fixture,
    order: Uuid,
    input: &CreateAdjustmentBatch,
) {
    let before = counts(pool).await;
    let preview = service.draft_preview(f.actor, None, input).await.unwrap();
    assert_eq!(
        service.draft_preview(f.actor, None, input).await.unwrap(),
        preview
    );
    assert_eq!(counts(pool).await, before);
    assert_eq!(preview["preview"]["effects"]["postsAdjustment"], false);
    assert_eq!(preview["preview"]["totalAmount"], "10.01");
    assert_eq!(
        preview["preview"]["referencedOrders"][0]["id"],
        json!(order)
    );
    let mut t = tx(pool).await;
    let mut wrong = preview.clone();
    wrong["preview"]["input"]["lines"][0]["amount"] = json!("0.01");
    assert!(service
        .apply_draft_preview_on(
            &mut t,
            f.actor,
            Uuid::new_v4(),
            "preview-tamper",
            None,
            input,
            &wrong
        )
        .await
        .is_err());
    t.rollback().await.unwrap();
    assert_eq!(counts(pool).await, before);
    // Confirmation cannot silently accept an order's changed version or dimensions.
    sqlx::query("UPDATE sales_orders SET order_date=order_date+1 WHERE id=$1")
        .bind(order)
        .execute(pool)
        .await
        .unwrap();
    let fresh = service.draft_preview(f.actor, None, input).await.unwrap();
    assert_ne!(fresh["previewHash"], preview["previewHash"]);
    let before = counts(pool).await;
    let mut t = tx(pool).await;
    assert!(service
        .apply_draft_preview_on(
            &mut t,
            f.actor,
            Uuid::new_v4(),
            "preview-stale-order",
            None,
            input,
            &preview
        )
        .await
        .is_err());
    t.rollback().await.unwrap();
    assert_eq!(counts(pool).await, before);
    let mut t = tx(pool).await;
    service
        .apply_draft_preview_on(
            &mut t,
            f.actor,
            Uuid::new_v4(),
            "preview-create-rollback",
            None,
            input,
            &fresh,
        )
        .await
        .unwrap();
    t.rollback().await.unwrap();
    assert_eq!(counts(pool).await, before);
    let mut t = tx(pool).await;
    let created = service
        .apply_draft_preview_on(
            &mut t,
            f.actor,
            Uuid::new_v4(),
            "preview-create-commit",
            None,
            input,
            &fresh,
        )
        .await
        .unwrap();
    t.commit().await.unwrap();
    assert_eq!(created.status, "draft");
    let mut many = input.clone();
    many.lines = vec![input.lines[0].clone(); 101];
    let many_draft = service
        .create(f.actor, Uuid::new_v4(), "preview-many-source", &many)
        .await
        .unwrap();
    let before = counts(pool).await;
    let replacement = service
        .draft_preview(f.actor, Some((many_draft.id, 1)), input)
        .await
        .unwrap();
    assert_eq!(
        replacement["preview"]["source"]["lines"]
            .as_array()
            .unwrap()
            .len(),
        101
    );
    assert_eq!(replacement["preview"]["effects"]["replacesAllLines"], true);
    assert_eq!(
        replacement["preview"]["input"]["lines"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(counts(pool).await, before);
    let mut t = tx(pool).await;
    let updated = service
        .apply_draft_preview_on(
            &mut t,
            f.actor,
            Uuid::new_v4(),
            "preview-replace-rollback",
            Some((many_draft.id, 1)),
            input,
            &replacement,
        )
        .await
        .unwrap();
    assert_eq!(updated.version, 2);
    t.rollback().await.unwrap();
    assert_eq!(counts(pool).await, before);
    let mut t = tx(pool).await;
    let updated = service
        .apply_draft_preview_on(
            &mut t,
            f.actor,
            Uuid::new_v4(),
            "preview-replace-commit",
            Some((many_draft.id, 1)),
            input,
            &replacement,
        )
        .await
        .unwrap();
    t.commit().await.unwrap();
    assert_eq!(updated.version, 2);
    assert!(service
        .draft_preview(f.actor, Some((many_draft.id, 1)), input)
        .await
        .is_err());
    // A current preview changes when any original line changes, including non-first pages.
    let source = service
        .create(f.actor, Uuid::new_v4(), "preview-last-line-source", &many)
        .await
        .unwrap();
    let prior = service
        .draft_preview(f.actor, Some((source.id, 1)), input)
        .await
        .unwrap();
    sqlx::query("UPDATE operational_adjustment_lines SET business_note='modified last line' WHERE batch_id=$1 AND line_number=101").bind(source.id).execute(pool).await.unwrap();
    let after = service
        .draft_preview(f.actor, Some((source.id, 1)), input)
        .await
        .unwrap();
    assert_ne!(prior["previewHash"], after["previewHash"]);
    let before = counts(pool).await;
    let mut t = tx(pool).await;
    assert!(service
        .apply_draft_preview_on(
            &mut t,
            f.actor,
            Uuid::new_v4(),
            "preview-hidden-line-change",
            Some((source.id, 1)),
            input,
            &prior
        )
        .await
        .is_err());
    t.rollback().await.unwrap();
    assert_eq!(counts(pool).await, before);
    let before = counts(pool).await;
    let mut denied = input.clone();
    denied.lines[0].customer_id = Some(Uuid::new_v4());
    assert!(service.draft_preview(f.actor, None, &denied).await.is_err());
    assert_eq!(counts(pool).await, before);
}
