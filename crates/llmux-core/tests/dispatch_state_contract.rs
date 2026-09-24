use llmux_core::adapters::Account;
use llmux_core::dispatcher::{DispatchRouter, DispatchStateRow};
use llmux_core::repo;

fn account(id: i64, name: &str) -> Account {
    Account {
        id,
        name: name.to_string(),
        vendor_id: "openai".to_string(),
        protocol: "openai".to_string(),
        api_key: "sk-test".to_string(),
        base_url: None,
        anthropic_base_url: None,
        custom_base_url: false,
        custom_anthropic_base_url: false,
        serves_anthropic: false,
        openai_compatible: 0,
        openai_responses: true,
        enabled: 1,
        weight: 10,
    }
}

fn epoch_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

/// 构造一行持久化状态。
fn row(key: &str, mode: &str, sticky: i64, last_probe_ms: i64, backoff: i64) -> DispatchStateRow {
    DispatchStateRow {
        dispatch_key: key.to_string(),
        mode: mode.to_string(),
        sticky_fallback_id: sticky,
        consecutive_successes: 0,
        last_probe_ms,
        probe_backoff_secs: backoff,
    }
}

#[test]
fn fallback_state_survives_snapshot_restore_round_trip() {
    let accounts = vec![account(1, "primary"), account(2, "backup")];
    let mut router = DispatchRouter::default();

    // 首选(1)失败、备用(2)接管 → 进入 Fallback
    let (_, meta) = router.select("alias:x", &accounts, 1);
    router.record_result("alias:x", &meta, Some(2), false);

    let rows = router.snapshot();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].mode, "fallback");
    assert_eq!(rows[0].sticky_fallback_id, 2);
    assert_eq!(rows[0].consecutive_successes, 1);
    assert_eq!(rows[0].probe_backoff_secs, 30);

    let restored = DispatchRouter::restore(rows);
    let again = restored.snapshot();
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].mode, "fallback");
    assert_eq!(again[0].sticky_fallback_id, 2);
    assert_eq!(again[0].probe_backoff_secs, 30);
}

#[test]
fn primary_entry_round_trips_as_primary() {
    let accounts = vec![account(1, "primary"), account(2, "backup")];
    let mut router = DispatchRouter::default();

    // 成功且命中首选 → 仍是 Primary，无状态变更
    let (_, meta) = router.select("alias:x", &accounts, 1);
    router.record_result("alias:x", &meta, Some(1), true);

    let rows = router.snapshot();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].mode, "primary");

    let restored = DispatchRouter::restore(rows);
    assert_eq!(restored.snapshot()[0].mode, "primary");
}

#[test]
fn long_downtime_expires_probe_backoff_after_restore() {
    // 停机 100s，backoff 仅 30s → 重启后首个请求应立即探测首选
    let restored = DispatchRouter::restore(vec![row(
        "alias:x",
        "fallback",
        2,
        epoch_millis() - 100_000,
        30,
    )]);
    let mut router = restored;
    let accounts = vec![account(1, "primary"), account(2, "backup")];

    let (ordered, meta) = router.select("alias:x", &accounts, 1);
    assert!(meta.is_probe, "停机时长已超过 backoff，应触发探测");
    assert_eq!(ordered[0].id, 1, "探测时首选排第一");
}

#[test]
fn fresh_probe_does_not_disturb_preferred_before_backoff() {
    // 5s 前刚探测过，backoff 600s → 不探测，首选排末位兜底
    let restored = DispatchRouter::restore(vec![row(
        "alias:x",
        "fallback",
        2,
        epoch_millis() - 5_000,
        600,
    )]);
    let mut router = restored;
    let accounts = vec![account(1, "primary"), account(2, "backup")];

    let (ordered, meta) = router.select("alias:x", &accounts, 1);
    assert!(!meta.is_probe, "退避未过期，不应探测");
    assert_eq!(ordered[0].id, 2, "sticky fallback 排第一");
    assert_eq!(ordered.last().unwrap().id, 1, "首选排最后兜底");
}

#[test]
fn future_last_probe_ms_does_not_panic() {
    // 时钟回拨：last_probe 落在未来 → 视为刚探测过，不 panic、不探测
    let restored = DispatchRouter::restore(vec![row(
        "alias:x",
        "fallback",
        2,
        epoch_millis() + 100_000,
        30,
    )]);
    let mut router = restored;
    let accounts = vec![account(1, "primary"), account(2, "backup")];

    let (_, meta) = router.select("alias:x", &accounts, 1);
    assert!(!meta.is_probe, "elapsed 被钳为 0，未达 backoff");
}

#[test]
fn primary_row_ignores_stale_probe_fields() {
    // Primary 行的 last_probe_ms 是 0（1970），不得被当成过期退避而误探测
    let restored = DispatchRouter::restore(vec![row("alias:x", "primary", 0, 0, 0)]);
    let mut router = restored;
    let accounts = vec![account(1, "primary"), account(2, "backup")];

    let (ordered, meta) = router.select("alias:x", &accounts, 1);
    assert!(!meta.is_probe);
    assert_eq!(ordered[0].id, 1, "Primary 模式首选排第一");
}

#[test]
fn unknown_mode_falls_back_to_primary() {
    let restored = DispatchRouter::restore(vec![row(
        "alias:x",
        "bogus-mode",
        2,
        epoch_millis() - 100_000,
        30,
    )]);
    let mut router = restored;
    let accounts = vec![account(1, "primary"), account(2, "backup")];

    let (ordered, meta) = router.select("alias:x", &accounts, 1);
    assert!(!meta.is_probe, "未知 mode 按 Primary 处理");
    assert_eq!(ordered[0].id, 1);
}

#[test]
fn sticky_fallback_id_pointing_to_deleted_account_does_not_panic() {
    // sticky 指向已不存在的账户 99 → 退化为原顺序，不得 panic
    let restored = DispatchRouter::restore(vec![row(
        "alias:x",
        "fallback",
        99,
        epoch_millis() - 1_000,
        600,
    )]);
    let mut router = restored;
    let accounts = vec![account(1, "primary"), account(2, "backup")];

    let (ordered, meta) = router.select("alias:x", &accounts, 1);
    assert!(!meta.is_probe);
    assert_eq!(ordered.len(), 2, "两个账户都还在，只是顺序退化");
}

#[test]
fn dirty_flag_lifecycle() {
    let accounts = vec![account(1, "primary"), account(2, "backup")];
    let mut router = DispatchRouter::default();
    assert!(!router.is_dirty());

    // 首次 select 只插入默认 Primary entry，不算变更
    let (_, meta) = router.select("alias:x", &accounts, 1);
    assert!(!router.is_dirty(), "插入默认 Primary 不应标脏");

    // 命中首选且成功 → 无变更
    router.record_result("alias:x", &meta, Some(1), true);
    assert!(!router.is_dirty(), "Primary 命中首选成功不应标脏");

    // 降级到 Fallback → 变更
    router.record_result("alias:x", &meta, Some(2), false);
    assert!(router.is_dirty(), "降级到 Fallback 应标脏");

    router.clear_dirty();
    assert!(!router.is_dirty(), "clear_dirty 后应为干净");
}

/// 建一个带完整 schema 的内存库（与 core_contract.rs 的 memory_db 一致）。
async fn memory_db() -> sqlx::SqlitePool {
    let mut pool = llmux_core::db::connect_sqlite("sqlite::memory:")
        .await
        .expect("connect memory sqlite");
    llmux_core::db::init_db(&mut pool, "sqlite::memory:")
        .await
        .expect("initialize schema");
    pool
}

#[tokio::test]
async fn save_and_load_dispatch_state_round_trip_and_full_rewrite() {
    let pool = memory_db().await;

    let rows = vec![
        row("alias:a", "fallback", 2, epoch_millis() - 1_000, 60),
        row("alias:b", "primary", 0, 0, 0),
    ];
    repo::save_dispatch_state(&pool, &rows).await.unwrap();

    let mut loaded = repo::load_dispatch_state(&pool).await.unwrap();
    loaded.sort_by(|a, b| a.dispatch_key.cmp(&b.dispatch_key));
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded[0].dispatch_key, "alias:a");
    assert_eq!(loaded[0].mode, "fallback");
    assert_eq!(loaded[0].sticky_fallback_id, 2);
    assert_eq!(loaded[0].probe_backoff_secs, 60);
    assert_eq!(loaded[1].dispatch_key, "alias:b");
    assert_eq!(loaded[1].mode, "primary");

    // 全量重写：只留一行，旧行必须被清除（淘汰对账依赖此语义）
    repo::save_dispatch_state(&pool, &rows[..1]).await.unwrap();
    let loaded = repo::load_dispatch_state(&pool).await.unwrap();
    assert_eq!(loaded.len(), 1, "全量重写应删除内存中已不存在的行");
    assert_eq!(loaded[0].dispatch_key, "alias:a");
}

#[tokio::test]
async fn load_dispatch_state_on_empty_table_returns_empty() {
    let pool = memory_db().await;
    let loaded = repo::load_dispatch_state(&pool).await.unwrap();
    assert!(loaded.is_empty());
}
