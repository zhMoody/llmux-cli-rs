use llmux_core::adapters::Account;
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

#[tokio::test]
async fn flush_once_persists_dirty_state_and_clears_flag() {
    let state = llmux_server::test_state().await;
    let accounts = vec![account(1, "primary"), account(2, "backup")];

    {
        let mut router = state.dispatch_router.lock().await;
        let (_, meta) = router.select("alias:x", &accounts, 1);
        router.record_result("alias:x", &meta, Some(2), false);
        assert!(router.is_dirty(), "降级后应处于脏状态");
    }

    let wrote = llmux_server::dispatch_flush::flush_once(&state).await;
    assert!(wrote, "脏状态应被落盘");

    {
        let router = state.dispatch_router.lock().await;
        assert!(!router.is_dirty(), "flush 后脏标记应被清除");
    }

    let rows = repo::load_dispatch_state(&state.pool).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].dispatch_key, "alias:x");
    assert_eq!(rows[0].mode, "fallback");
    assert_eq!(rows[0].sticky_fallback_id, 2);
}

#[tokio::test]
async fn flush_once_is_noop_when_clean() {
    let state = llmux_server::test_state().await;
    let wrote = llmux_server::dispatch_flush::flush_once(&state).await;
    assert!(!wrote, "无变更时不应写库");
    let rows = repo::load_dispatch_state(&state.pool).await.unwrap();
    assert!(rows.is_empty());
}
