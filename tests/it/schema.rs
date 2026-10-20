use crate::support;
use xuewen::library::projects::{self, ProjectName};
use xuewen::library::tags::{self, TagName};
use xuewen::library::{papers, trash};

#[tokio::test]
async fn deleting_a_paper_cascades_chat_messages() {
    let (pool, _root) = support::pool_and_root_with_paper("p1").await;
    xuewen::agent::history::insert_exchange(&pool, "p1", "q", "a", "M", None)
        .await
        .unwrap();
    // 0022 rebuilt chat_messages with ON DELETE CASCADE; before it this
    // delete FK-failed unless the caller cleared the chat rows first.
    trash::hard_delete(&pool, "p1").await.unwrap();
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chat_messages WHERE paper_id = ?")
        .bind("p1")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[tokio::test]
async fn membership_add_remove_and_filter_and_cascade() {
    let (_dir, pool) = support::temp_pool().await;
    papers::insert(
        &pool,
        &support::sample_paper("01890000-0000-7000-8000-0000000000a1", "ha"),
    )
    .await
    .unwrap();
    papers::insert(
        &pool,
        &support::sample_paper("01890000-0000-7000-8000-0000000000a2", "hb"),
    )
    .await
    .unwrap();
    let proj = projects::create(&pool, &ProjectName::parse("P").unwrap())
        .await
        .unwrap();

    // Add is idempotent.
    projects::add_paper(&pool, "01890000-0000-7000-8000-0000000000a1", &proj.id)
        .await
        .unwrap();
    projects::add_paper(&pool, "01890000-0000-7000-8000-0000000000a1", &proj.id)
        .await
        .unwrap();
    assert_eq!(
        projects::ids_for_paper(&pool, "01890000-0000-7000-8000-0000000000a1")
            .await
            .unwrap(),
        vec![proj.id.clone()]
    );

    // Count reflects membership.
    assert_eq!(projects::list(&pool).await.unwrap()[0].paper_count, 1);

    // Soft-deleting a member drops the count (it matches the ?project= filter,
    // which hides trashed papers) while the membership row itself survives.
    trash::soft_delete(&pool, "01890000-0000-7000-8000-0000000000a1")
        .await
        .unwrap();
    assert_eq!(projects::list(&pool).await.unwrap()[0].paper_count, 0);
    assert_eq!(
        projects::ids_for_paper(&pool, "01890000-0000-7000-8000-0000000000a1")
            .await
            .unwrap(),
        vec![proj.id.clone()]
    );
    // Restoring it brings the count back.
    trash::restore(&pool, "01890000-0000-7000-8000-0000000000a1")
        .await
        .unwrap();
    assert_eq!(projects::list(&pool).await.unwrap()[0].paper_count, 1);

    // Filter returns only members.
    let member_filter = papers::PaperFilter {
        project: Some(proj.id.clone()),
        ..Default::default()
    };
    let filtered = papers::list(&pool, None, None, &member_filter)
        .await
        .unwrap();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].id, "01890000-0000-7000-8000-0000000000a1");

    // Remove.
    assert!(
        projects::remove_paper(&pool, "01890000-0000-7000-8000-0000000000a1", &proj.id)
            .await
            .unwrap()
    );
    assert!(
        !projects::remove_paper(&pool, "01890000-0000-7000-8000-0000000000a1", &proj.id)
            .await
            .unwrap()
    );

    // FK cascade: hard-purging a paper drops its memberships.
    projects::add_paper(&pool, "01890000-0000-7000-8000-0000000000a2", &proj.id)
        .await
        .unwrap();
    trash::hard_delete(&pool, "01890000-0000-7000-8000-0000000000a2")
        .await
        .unwrap();
    assert_eq!(projects::list(&pool).await.unwrap()[0].paper_count, 0);

    // FK cascade: deleting a project drops memberships (no orphan rows).
    projects::add_paper(&pool, "01890000-0000-7000-8000-0000000000a1", &proj.id)
        .await
        .unwrap();
    projects::delete(&pool, &proj.id).await.unwrap();
    assert!(
        projects::ids_for_paper(&pool, "01890000-0000-7000-8000-0000000000a1")
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn batched_memberships_group_by_paper_and_keep_order() {
    let (_dir, pool) = support::temp_pool().await;
    papers::insert(&pool, &support::sample_paper("pa", "ha"))
        .await
        .unwrap();
    papers::insert(&pool, &support::sample_paper("pb", "hb"))
        .await
        .unwrap();
    papers::insert(&pool, &support::sample_paper("pc", "hc"))
        .await
        .unwrap();

    // Tags: NOCASE-sorted per paper, papers without any absent from the map.
    tags::add_paper(&pool, "pa", &TagName::parse("zeta").unwrap())
        .await
        .unwrap();
    tags::add_paper(&pool, "pa", &TagName::parse("Alpha").unwrap())
        .await
        .unwrap();
    tags::add_paper(&pool, "pb", &TagName::parse("zeta").unwrap())
        .await
        .unwrap();
    let tags = tags::for_papers(
        &pool,
        &["pa".to_string(), "pb".to_string(), "pc".to_string()],
    )
    .await
    .unwrap();
    let names = |id: &str| -> Vec<String> {
        tags.get(id)
            .map(|ts| ts.iter().map(|t| t.name.clone()).collect())
            .unwrap_or_default()
    };
    assert_eq!(names("pa"), vec!["Alpha".to_string(), "zeta".to_string()]);
    assert_eq!(names("pb"), vec!["zeta".to_string()]);
    assert!(!tags.contains_key("pc"));

    // Must agree with the per-paper query it batches.
    assert_eq!(tags["pa"], tags::for_paper(&pool, "pa").await.unwrap());

    // Projects: join order per paper, mirroring projects::for_paper.
    let p1 = projects::create(&pool, &ProjectName::parse("First").unwrap())
        .await
        .unwrap();
    let p2 = projects::create(&pool, &ProjectName::parse("Second").unwrap())
        .await
        .unwrap();
    projects::add_paper(&pool, "pa", &p1.id).await.unwrap();
    projects::add_paper(&pool, "pa", &p2.id).await.unwrap();
    let projects = projects::for_papers(&pool, &["pa".to_string(), "pb".to_string()])
        .await
        .unwrap();
    assert_eq!(
        projects["pa"],
        projects::for_paper(&pool, "pa").await.unwrap()
    );
    assert!(!projects.contains_key("pb"));

    // Empty input short-circuits (no `IN ()` SQL).
    assert!(tags::for_papers(&pool, &[]).await.unwrap().is_empty());
    assert!(projects::for_papers(&pool, &[]).await.unwrap().is_empty());
}
