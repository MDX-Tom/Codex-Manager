use super::*;
use crate::storage::{now_ts, Account};

fn sample_account(id: &str, now: i64) -> Account {
    Account {
        id: id.to_string(),
        label: id.to_string(),
        issuer: "issuer".to_string(),
        chatgpt_account_id: None,
        workspace_id: None,
        group_name: None,
        sort: 0,
        status: "active".to_string(),
        created_at: now,
        updated_at: now,
    }
}

fn sample_snapshot(account_id: &str, captured_at: i64, used_percent: f64) -> UsageSnapshotRecord {
    UsageSnapshotRecord {
        account_id: account_id.to_string(),
        used_percent: Some(used_percent),
        window_minutes: Some(180),
        resets_at: None,
        secondary_used_percent: None,
        secondary_window_minutes: None,
        secondary_resets_at: None,
        credits_json: None,
        captured_at,
    }
}

fn collect_query_plan(storage: &Storage, sql: &str) -> String {
    collect_query_plan_with_params(storage, sql, Vec::new())
}

fn collect_query_plan_with_params(
    storage: &Storage,
    sql: &str,
    params: Vec<rusqlite::types::Value>,
) -> String {
    let mut stmt = storage.conn.prepare(sql).expect("prepare explain");
    let mut rows = stmt.query(params_from_iter(params)).expect("query explain");
    let mut plan = String::new();
    while let Some(row) = rows.next().expect("read explain row") {
        let detail: String = row.get(3).expect("plan detail");
        plan.push_str(&detail);
        plan.push('\n');
    }
    plan
}

#[test]
fn latest_usage_snapshots_for_accounts_filters_and_returns_latest_per_account() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    for account_id in ["acc-a", "acc-b", "acc-c"] {
        storage
            .insert_account(&sample_account(account_id, now))
            .expect("insert account");
    }
    storage
        .insert_usage_snapshot(&sample_snapshot("acc-a", now, 10.0))
        .expect("insert old a");
    storage
        .insert_usage_snapshot(&sample_snapshot("acc-a", now + 1, 20.0))
        .expect("insert new a");
    storage
        .insert_usage_snapshot(&sample_snapshot("acc-b", now + 2, 30.0))
        .expect("insert b");
    storage
        .insert_usage_snapshot(&sample_snapshot("acc-c", now + 3, 40.0))
        .expect("insert c");

    let requested = vec![
        "acc-a".to_string(),
        "acc-c".to_string(),
        "missing".to_string(),
    ];
    let items = storage
        .latest_usage_snapshots_for_accounts(&requested)
        .expect("list snapshots");
    let by_account = items
        .into_iter()
        .map(|item| (item.account_id.clone(), item))
        .collect::<std::collections::HashMap<_, _>>();

    assert_eq!(by_account.len(), 2);
    assert_eq!(
        by_account.get("acc-a").and_then(|item| item.used_percent),
        Some(20.0)
    );
    assert_eq!(
        by_account.get("acc-c").and_then(|item| item.used_percent),
        Some(40.0)
    );
    assert!(!by_account.contains_key("acc-b"));
}

#[test]
fn latest_usage_snapshots_by_account_limited_returns_recent_latest_snapshots() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    for account_id in ["acc-a", "acc-b", "acc-c"] {
        storage
            .insert_account(&sample_account(account_id, now))
            .expect("insert account");
    }
    storage
        .insert_usage_snapshot(&sample_snapshot("acc-a", now, 10.0))
        .expect("insert old a");
    storage
        .insert_usage_snapshot(&sample_snapshot("acc-a", now + 5, 15.0))
        .expect("insert latest a");
    storage
        .insert_usage_snapshot(&sample_snapshot("acc-b", now + 3, 30.0))
        .expect("insert b");
    storage
        .insert_usage_snapshot(&sample_snapshot("acc-c", now + 4, 40.0))
        .expect("insert c");

    let items = storage
        .latest_usage_snapshots_by_account_limited(Some(2))
        .expect("list limited snapshots");

    assert_eq!(
        items
            .iter()
            .map(|item| (item.account_id.as_str(), item.used_percent))
            .collect::<Vec<_>>(),
        vec![("acc-a", Some(15.0)), ("acc-c", Some(40.0))]
    );
}

#[test]
fn latest_usage_snapshots_by_account_limited_zero_returns_empty() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();
    storage
        .insert_account(&sample_account("acc-zero", now))
        .expect("insert account");
    storage
        .insert_usage_snapshot(&sample_snapshot("acc-zero", now, 10.0))
        .expect("insert usage snapshot");

    let items = storage
        .latest_usage_snapshots_by_account_limited(Some(0))
        .expect("list zero limited snapshots");

    assert!(items.is_empty());
}

#[test]
fn latest_usage_snapshot_with_extra_rate_limits_skips_empty_latest_bucket() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();
    storage
        .insert_account(&sample_account("acc-extra-history", now))
        .expect("insert account");

    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: "acc-extra-history".to_string(),
            used_percent: Some(100.0),
            window_minutes: Some(300),
            resets_at: None,
            secondary_used_percent: Some(100.0),
            secondary_window_minutes: Some(10080),
            secondary_resets_at: None,
            credits_json: Some(
                r#"{"_codexmanager_extra_rate_limits":[{"limit_name":"gpt-reserve"}]}"#.to_string(),
            ),
            captured_at: now,
        })
        .expect("insert extra snapshot");
    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: "acc-extra-history".to_string(),
            credits_json: Some(r#"{"_codexmanager_extra_rate_limits":[]}"#.to_string()),
            captured_at: now + 1,
            ..sample_snapshot("acc-extra-history", now + 1, 100.0)
        })
        .expect("insert empty snapshot");

    let recovered = storage
        .latest_usage_snapshot_with_extra_rate_limits_for_account("acc-extra-history")
        .expect("read extra snapshot")
        .expect("extra snapshot exists");
    assert_eq!(recovered.captured_at, now);
    assert!(recovered
        .credits_json
        .as_deref()
        .is_some_and(|json| json.contains("gpt-reserve")));
}

#[test]
fn latest_usage_quota_source_rows_for_accounts_reads_only_quota_source_fields() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    for account_id in ["acc-a", "acc-b", "acc-c"] {
        storage
            .insert_account(&sample_account(account_id, now))
            .expect("insert account");
    }
    storage
        .insert_usage_snapshot(&sample_snapshot("acc-a", now, 10.0))
        .expect("insert old a");
    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            window_minutes: Some(999),
            resets_at: Some(now + 999),
            secondary_used_percent: Some(88.0),
            secondary_window_minutes: Some(10080),
            secondary_resets_at: Some(now + 10080),
            credits_json: Some(r#"{"planType":"plus"}"#.to_string()),
            ..sample_snapshot("acc-a", now + 5, 15.0)
        })
        .expect("insert latest a");
    storage
        .insert_usage_snapshot(&sample_snapshot("acc-b", now + 3, 30.0))
        .expect("insert b");
    storage
        .insert_usage_snapshot(&sample_snapshot("acc-c", now + 4, 40.0))
        .expect("insert c");

    let requested = vec![
        "acc-b".to_string(),
        "missing".to_string(),
        "acc-a".to_string(),
    ];
    let rows = storage
        .latest_usage_quota_source_rows_for_accounts(&requested)
        .expect("list quota source rows");

    assert_eq!(
        rows.iter()
            .map(|row| (
                row.account_id.as_str(),
                row.used_percent,
                row.secondary_used_percent,
                row.captured_at
            ))
            .collect::<Vec<_>>(),
        vec![
            ("acc-a", Some(15.0), Some(88.0), now + 5),
            ("acc-b", Some(30.0), None, now + 3)
        ]
    );
}

#[test]
fn latest_usage_cleanup_rows_for_accounts_reads_cleanup_fields_only() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    for account_id in ["acc-a", "acc-b", "acc-c"] {
        storage
            .insert_account(&sample_account(account_id, now))
            .expect("insert account");
    }
    storage
        .insert_usage_snapshot(&sample_snapshot("acc-a", now, 10.0))
        .expect("insert old a");
    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            window_minutes: Some(999),
            resets_at: Some(now + 999),
            secondary_used_percent: Some(88.0),
            secondary_window_minutes: Some(10080),
            secondary_resets_at: Some(now + 10080),
            credits_json: Some(r#"{"planType":"free"}"#.to_string()),
            ..sample_snapshot("acc-a", now + 5, 15.0)
        })
        .expect("insert latest a");
    storage
        .insert_usage_snapshot(&sample_snapshot("acc-b", now + 3, 30.0))
        .expect("insert b");
    storage
        .insert_usage_snapshot(&sample_snapshot("acc-c", now + 4, 40.0))
        .expect("insert c");

    let rows = storage
        .latest_usage_cleanup_rows_for_accounts(&[
            "acc-b".to_string(),
            "missing".to_string(),
            "acc-a".to_string(),
        ])
        .expect("list cleanup rows");

    assert_eq!(
        rows.iter()
            .map(|row| (
                row.account_id.as_str(),
                row.used_percent,
                row.window_minutes,
                row.secondary_used_percent,
                row.secondary_window_minutes,
                row.credits_json.as_deref()
            ))
            .collect::<Vec<_>>(),
        vec![
            (
                "acc-a",
                Some(15.0),
                Some(999),
                Some(88.0),
                Some(10080),
                Some(r#"{"planType":"free"}"#)
            ),
            ("acc-b", Some(30.0), Some(180), None, None, None)
        ]
    );
}

#[test]
fn low_quota_account_ids_for_accounts_filters_latest_snapshot_in_sql() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    for account_id in [
        "acc-primary-low",
        "acc-secondary-low",
        "acc-recovered",
        "acc-ok",
    ] {
        storage
            .insert_account(&sample_account(account_id, now))
            .expect("insert account");
    }
    storage
        .insert_usage_snapshot(&sample_snapshot("acc-primary-low", now, 96.0))
        .expect("insert primary low");
    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            secondary_used_percent: Some(92.0),
            secondary_window_minutes: Some(10_080),
            ..sample_snapshot("acc-secondary-low", now + 1, 10.0)
        })
        .expect("insert secondary low");
    storage
        .insert_usage_snapshot(&sample_snapshot("acc-recovered", now + 2, 99.0))
        .expect("insert old recovered low");
    storage
        .insert_usage_snapshot(&sample_snapshot("acc-recovered", now + 3, 20.0))
        .expect("insert latest recovered ok");
    storage
        .insert_usage_snapshot(&sample_snapshot("acc-ok", now + 4, 50.0))
        .expect("insert ok");

    let account_ids = vec![
        "acc-ok".to_string(),
        "acc-secondary-low".to_string(),
        "acc-primary-low".to_string(),
        "acc-recovered".to_string(),
        "missing".to_string(),
    ];
    let low_quota = storage
        .low_quota_account_ids_for_accounts(&account_ids, 5.0, 10.0)
        .expect("list low quota accounts");

    assert_eq!(
        low_quota,
        vec![
            "acc-primary-low".to_string(),
            "acc-secondary-low".to_string()
        ]
    );
}

#[test]
fn low_quota_account_ids_match_thresholds_by_window_duration() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    for account_id in ["acc-swapped", "acc-long-only"] {
        storage
            .insert_account(&sample_account(account_id, now))
            .expect("insert account");
    }
    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: "acc-swapped".to_string(),
            used_percent: Some(90.0),
            window_minutes: Some(10_080),
            resets_at: None,
            secondary_used_percent: Some(90.0),
            secondary_window_minutes: Some(300),
            secondary_resets_at: None,
            credits_json: None,
            captured_at: now,
        })
        .expect("insert swapped windows");
    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: "acc-long-only".to_string(),
            used_percent: Some(90.0),
            window_minutes: Some(10_080),
            resets_at: None,
            secondary_used_percent: None,
            secondary_window_minutes: None,
            secondary_resets_at: None,
            credits_json: None,
            captured_at: now,
        })
        .expect("insert long-only window");

    let low_quota = storage
        .low_quota_account_ids_for_accounts(
            &["acc-swapped".to_string(), "acc-long-only".to_string()],
            15.0,
            5.0,
        )
        .expect("list low quota accounts");

    assert_eq!(low_quota, vec!["acc-swapped".to_string()]);
}

#[test]
fn low_quota_account_ids_for_accounts_returns_empty_when_thresholds_disabled() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();
    storage
        .insert_account(&sample_account("acc-low", now))
        .expect("insert account");
    storage
        .insert_usage_snapshot(&sample_snapshot("acc-low", now, 100.0))
        .expect("insert low");

    let low_quota = storage
        .low_quota_account_ids_for_accounts(&["acc-low".to_string()], 0.0, -5.0)
        .expect("list low quota accounts");

    assert!(low_quota.is_empty());
}

#[test]
fn low_quota_account_ids_for_accounts_chunks_large_account_sets() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();
    let target = "acc-0949";
    storage
        .insert_account(&sample_account(target, now))
        .expect("insert target account");
    storage
        .insert_usage_snapshot(&sample_snapshot(target, now, 98.0))
        .expect("insert target low");

    let account_ids = (0..950)
        .map(|index| format!("acc-{index:04}"))
        .collect::<Vec<_>>();
    let low_quota = storage
        .low_quota_account_ids_for_accounts(&account_ids, 5.0, 0.0)
        .expect("list low quota accounts");

    assert_eq!(low_quota, vec![target.to_string()]);
}

#[test]
fn usage_account_chunk_queries_defer_final_ordering_to_rust() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    let account_condition = "account_id IN ('acc-a', 'acc-b')";
    let latest_sql = latest_usage_snapshots_for_accounts_chunk_sql(account_condition);
    let latest_plan = collect_query_plan(&storage, &format!("EXPLAIN QUERY PLAN {latest_sql}"));
    let quota_source_sql = latest_usage_quota_source_rows_for_accounts_chunk_sql(account_condition);
    let quota_source_plan =
        collect_query_plan(&storage, &format!("EXPLAIN QUERY PLAN {quota_source_sql}"));
    let cleanup_sql = latest_usage_cleanup_rows_for_accounts_chunk_sql(account_condition);
    let cleanup_plan = collect_query_plan(&storage, &format!("EXPLAIN QUERY PLAN {cleanup_sql}"));
    let low_quota_sql = low_quota_account_ids_for_accounts_chunk_sql(account_condition);
    let low_quota_plan = collect_query_plan_with_params(
        &storage,
        &format!("EXPLAIN QUERY PLAN {low_quota_sql}"),
        vec![rusqlite::types::Value::Real(5.0); 8],
    );

    for (label, plan) in [
        ("latest usage chunk query", &latest_plan),
        ("quota source chunk query", &quota_source_plan),
        ("usage cleanup chunk query", &cleanup_plan),
        ("low quota chunk query", &low_quota_plan),
    ] {
        assert!(
            plan.contains("idx_usage_snapshots_account_captured_id"),
            "{label} should use account captured lookup index, got {plan}"
        );
    }

    assert!(
        !latest_plan.contains("USE TEMP B-TREE FOR ORDER BY"),
        "latest usage chunk output should not require an outer per-chunk sort, got {latest_plan}"
    );
    assert!(
        !quota_source_plan.contains("USE TEMP B-TREE FOR ORDER BY"),
        "quota source chunk output should not require an outer per-chunk sort, got {quota_source_plan}"
    );
    assert!(
        !cleanup_plan.contains("USE TEMP B-TREE FOR ORDER BY"),
        "usage cleanup chunk output should not require an outer per-chunk sort, got {cleanup_plan}"
    );
    assert!(
        !low_quota_plan.contains("USE TEMP B-TREE FOR ORDER BY"),
        "low quota chunk output should not require an outer per-chunk sort, got {low_quota_plan}"
    );
}

#[test]
fn latest_usage_snapshot_lookup_helpers_use_existing_indexes() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    let latest_plan = collect_query_plan(
        &storage,
        &format!("EXPLAIN QUERY PLAN {}", latest_usage_snapshot_sql()),
    );
    let latest_for_account_plan = collect_query_plan_with_params(
        &storage,
        &format!(
            "EXPLAIN QUERY PLAN {}",
            latest_usage_snapshot_for_account_sql()
        ),
        vec![rusqlite::types::Value::Text("acc-a".to_string())],
    );
    let count_for_account_plan = collect_query_plan_with_params(
        &storage,
        &format!(
            "EXPLAIN QUERY PLAN {}",
            usage_snapshot_count_for_account_sql()
        ),
        vec![rusqlite::types::Value::Text("acc-a".to_string())],
    );
    let summary_sql = latest_usage_snapshot_summary_rows_sql();
    let summary_plan = collect_query_plan(&storage, &format!("EXPLAIN QUERY PLAN {summary_sql}"));
    let by_account_sql = latest_usage_snapshots_by_account_sql(Some(10));
    let by_account_plan = collect_query_plan_with_params(
        &storage,
        &format!("EXPLAIN QUERY PLAN {by_account_sql}"),
        vec![rusqlite::types::Value::Integer(10)],
    );

    assert!(
        latest_plan.contains("idx_usage_snapshots_captured_id"),
        "latest usage snapshot should use captured/id ordering index, got {latest_plan}"
    );
    assert!(
        latest_for_account_plan.contains("idx_usage_snapshots_account_captured_id"),
        "account latest usage snapshot should use account captured lookup index, got {latest_for_account_plan}"
    );
    assert!(
        count_for_account_plan.contains("idx_usage_snapshots_account_captured_id"),
        "account usage count should use account captured lookup index, got {count_for_account_plan}"
    );
    assert!(
        summary_plan.contains("idx_usage_snapshots_account_captured_id"),
        "summary usage query should use account captured lookup index, got {summary_plan}"
    );
    assert!(
        by_account_plan.contains("idx_usage_snapshots_account_captured_id"),
        "latest usage snapshots by account should use account captured lookup index, got {by_account_plan}"
    );

    for (label, plan) in [
        ("latest usage snapshot", &latest_plan),
        ("account latest usage snapshot", &latest_for_account_plan),
        ("summary usage query", &summary_plan),
    ] {
        assert!(
            !plan.contains("USE TEMP B-TREE FOR ORDER BY"),
            "{label} should read in index order without temp ordering, got {plan}"
        );
    }
}
#[test]
fn latest_usage_snapshot_summary_rows_return_latest_usage_fields() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: "acc-summary".to_string(),
            used_percent: Some(10.0),
            window_minutes: Some(180),
            resets_at: Some(now + 180),
            secondary_used_percent: None,
            secondary_window_minutes: None,
            secondary_resets_at: None,
            credits_json: None,
            captured_at: now,
        })
        .expect("insert old summary snapshot");
    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: "acc-summary".to_string(),
            used_percent: Some(25.0),
            window_minutes: Some(240),
            resets_at: Some(now + 240),
            secondary_used_percent: Some(40.0),
            secondary_window_minutes: Some(10080),
            secondary_resets_at: Some(now + 10080),
            credits_json: Some(r#"{"planType":"free"}"#.to_string()),
            captured_at: now + 1,
        })
        .expect("insert new summary snapshot");

    let rows = storage
        .latest_usage_snapshot_summary_rows()
        .expect("list summary rows");

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].account_id, "acc-summary");
    assert_eq!(rows[0].used_percent, Some(25.0));
    assert_eq!(rows[0].window_minutes, Some(240));
    assert_eq!(rows[0].secondary_used_percent, Some(40.0));
    assert_eq!(rows[0].secondary_window_minutes, Some(10080));
    assert_eq!(
        rows[0].credits_json.as_deref(),
        Some(r#"{"planType":"free"}"#)
    );
}

#[test]
fn usage_snapshot_count_counts_all_snapshot_rows() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    storage
        .insert_usage_snapshot(&sample_snapshot("acc-a", now, 10.0))
        .expect("insert a snapshot");
    storage
        .insert_usage_snapshot(&sample_snapshot("acc-a", now + 1, 20.0))
        .expect("insert second a snapshot");
    storage
        .insert_usage_snapshot(&sample_snapshot("acc-b", now + 2, 30.0))
        .expect("insert b snapshot");

    assert_eq!(storage.usage_snapshot_count().expect("count snapshots"), 3);
}

#[test]
fn latest_usage_snapshots_for_accounts_chunks_large_account_sets() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    let target = "acc-0949";
    storage
        .insert_account(&sample_account(target, now))
        .expect("insert target account");
    storage
        .insert_usage_snapshot(&sample_snapshot(target, now, 45.0))
        .expect("insert old target");
    storage
        .insert_usage_snapshot(&sample_snapshot(target, now + 1, 55.0))
        .expect("insert new target");

    let requested = (0..950)
        .map(|index| format!("acc-{index:04}"))
        .collect::<Vec<_>>();
    let items = storage
        .latest_usage_snapshots_for_accounts(&requested)
        .expect("list snapshots");

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].account_id, target);
    assert_eq!(items[0].used_percent, Some(55.0));
}
#[test]
fn usage_snapshot_prune_helpers_use_existing_indexes() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    let account_prune_plan = collect_query_plan_with_params(
        &storage,
        &format!(
            "EXPLAIN QUERY PLAN {}",
            prune_usage_snapshots_for_account_sql()
        ),
        vec![
            rusqlite::types::Value::Text("acc-a".to_string()),
            rusqlite::types::Value::Integer(1),
        ],
    );
    let global_prune_plan = collect_query_plan_with_params(
        &storage,
        &format!(
            "EXPLAIN QUERY PLAN {}",
            prune_usage_snapshots_all_accounts_sql()
        ),
        vec![rusqlite::types::Value::Integer(1)],
    );

    assert!(
        account_prune_plan.contains("idx_usage_snapshots_account_captured_id"),
        "account usage snapshot prune should use account captured lookup index, got {account_prune_plan}"
    );
    assert!(
        global_prune_plan.contains("idx_usage_snapshots_account_captured_id"),
        "global usage snapshot prune should use account captured ordering index, got {global_prune_plan}"
    );
}

#[test]
fn usage_snapshot_delete_for_account_uses_account_index() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    let plan = collect_query_plan_with_params(
        &storage,
        &format!(
            "EXPLAIN QUERY PLAN {}",
            delete_usage_snapshots_for_account_sql()
        ),
        vec![rusqlite::types::Value::Text("acc-a".to_string())],
    );

    assert!(
        plan.contains("idx_usage_snapshots_account_captured_id"),
        "usage snapshot delete should use account captured lookup index, got {plan}"
    );
}
