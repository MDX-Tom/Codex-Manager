use super::{
    build_usage_refresh_task_plans, build_usage_refresh_task_plans_from_targets,
    build_usage_refresh_tasks, load_refreshable_accounts,
};
use codexmanager_core::storage::{
    now_ts, Account, AccountTokenCandidate, AccountUsageRefreshTarget, Storage, Token,
};
use std::collections::HashSet;

/// 函数 `account`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - id: 参数 id
/// - status: 参数 status
/// - workspace_id: 参数 workspace_id
///
/// # 返回
/// 返回函数执行结果
fn refresh_target(id: &str, status: &str, workspace_id: Option<&str>) -> AccountUsageRefreshTarget {
    AccountUsageRefreshTarget {
        id: id.to_string(),
        status: status.to_string(),
        workspace_id: workspace_id.map(|value| value.to_string()),
    }
}

fn account(id: &str, status: &str, workspace_id: Option<&str>) -> Account {
    Account {
        id: id.to_string(),
        label: "ignored label".to_string(),
        issuer: "ignored issuer".to_string(),
        chatgpt_account_id: Some("ignored-chatgpt-account".to_string()),
        workspace_id: workspace_id.map(|value| value.to_string()),
        group_name: Some("ignored group".to_string()),
        sort: 0,
        status: status.to_string(),
        created_at: now_ts(),
        updated_at: now_ts(),
    }
}

/// 函数 `token`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - account_id: 参数 account_id
///
/// # 返回
/// 返回函数执行结果
fn token(account_id: &str) -> Token {
    Token {
        account_id: account_id.to_string(),
        id_token: "id-token".to_string(),
        access_token: "access-token".to_string(),
        refresh_token: "refresh-token".to_string(),
        api_key_access_token: None,
        last_refresh: now_ts(),
    }
}

/// 函数 `build_usage_refresh_tasks_skips_disabled_and_banned_accounts`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn build_usage_refresh_tasks_skips_disabled_and_banned_accounts() {
    let tasks = build_usage_refresh_tasks(
        vec![
            token("acc-active"),
            token("acc-disabled"),
            token("acc-banned"),
            token("acc-inactive"),
            token("acc-unavailable"),
            token("acc-missing"),
        ],
        &[
            refresh_target("acc-active", "active", Some("ws-active")),
            refresh_target("acc-disabled", "disabled", Some("ws-disabled")),
            refresh_target("acc-banned", "banned", Some("ws-banned")),
            refresh_target("acc-inactive", "inactive", Some("ws-inactive")),
            refresh_target("acc-unavailable", "unavailable", Some("ws-unavailable")),
        ],
        &HashSet::from([String::from("acc-banned")]),
    );

    let account_ids = tasks
        .iter()
        .map(|task| task.account_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        account_ids,
        vec!["acc-active", "acc-inactive", "acc-unavailable"]
    );
    assert_eq!(tasks[0].workspace_id.as_deref(), Some("ws-active"));
    assert_eq!(tasks[1].workspace_id.as_deref(), Some("ws-inactive"));
    assert_eq!(tasks[2].workspace_id.as_deref(), Some("ws-unavailable"));
}

#[test]
fn build_usage_refresh_task_plans_accepts_only_usable_token_candidates() {
    let tasks = build_usage_refresh_task_plans(
        vec![
            AccountTokenCandidate {
                account_id: "acc-ready".to_string(),
                has_access_token: true,
                has_refresh_token: true,
                last_refresh: 1,
            },
            AccountTokenCandidate {
                account_id: "acc-no-access".to_string(),
                has_access_token: false,
                has_refresh_token: true,
                last_refresh: 2,
            },
            AccountTokenCandidate {
                account_id: "acc-no-refresh".to_string(),
                has_access_token: true,
                has_refresh_token: false,
                last_refresh: 3,
            },
        ],
        &[
            refresh_target("acc-ready", "active", Some("ws-ready")),
            refresh_target("acc-no-access", "active", Some("ws-no-access")),
            refresh_target("acc-no-refresh", "active", Some("ws-no-refresh")),
        ],
        &HashSet::new(),
    );

    let account_ids = tasks
        .iter()
        .map(|task| task.account_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(account_ids, vec!["acc-ready"]);
    assert_eq!(tasks[0].workspace_id.as_deref(), Some("ws-ready"));
}

#[test]
fn build_usage_refresh_task_plans_from_targets_skips_blocked_targets() {
    let tasks = build_usage_refresh_task_plans_from_targets(
        &[
            refresh_target("acc-ready", "active", Some("ws-ready")),
            refresh_target("acc-disabled", "disabled", Some("ws-disabled")),
            refresh_target("acc-banned-reason", "active", Some("ws-banned")),
        ],
        &HashSet::from([String::from("acc-banned-reason")]),
    );

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].account_id, "acc-ready");
    assert_eq!(tasks[0].workspace_id.as_deref(), Some("ws-ready"));
}

#[test]
fn load_refreshable_accounts_skips_disabled_and_banned_rows_in_sql() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    for account in [
        account("acc-active", "active", None),
        account("acc-inactive", "inactive", None),
        account("acc-limited", "limited", None),
        account("acc-unavailable", "unavailable", None),
        account("acc-unknown", "unknown", None),
        account("acc-force-enabled", "force_enabled", None),
        account("acc-disabled", "disabled", None),
        account("acc-banned", "banned", None),
    ] {
        storage.insert_account(&account).expect("insert account");
    }

    let account_ids = load_refreshable_accounts(&storage)
        .expect("load refreshable accounts")
        .into_iter()
        .map(|account| account.id)
        .collect::<Vec<_>>();

    assert_eq!(
        account_ids,
        vec![
            "acc-active".to_string(),
            "acc-force-enabled".to_string(),
            "acc-inactive".to_string(),
            "acc-limited".to_string(),
            "acc-unavailable".to_string(),
            "acc-unknown".to_string()
        ]
    );
}
