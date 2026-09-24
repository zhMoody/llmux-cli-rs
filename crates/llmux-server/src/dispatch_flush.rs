//! 把 DispatchRouter 的粘滞路由状态定期落盘的后台任务。
//!
//! 设计要点：router 全局锁内只做内存快照，DB 写入一律在锁外完成，
//! 避免持锁做 IO 拖慢 /v1 请求的调度路径。

use crate::app::AppState;
use std::time::Duration;
use tokio::time::interval;

/// 落盘间隔。停机最多丢一个周期的状态，而退避最小单位为 30s，影响可忽略。
const FLUSH_INTERVAL_SECS: u64 = 1;

/// 执行一次 flush：若 router 处于脏状态则落盘，返回是否写入了数据。
///
/// 快照在锁内取得、脏标记在锁内清除，随后释放锁再写库。
pub async fn flush_once(state: &AppState) -> bool {
    let rows = {
        let mut router = state.dispatch_router.lock().await;
        if !router.is_dirty() {
            return false;
        }
        let rows = router.snapshot();
        router.clear_dirty();
        rows
    };

    if let Err(e) = llmux_core::repo::save_dispatch_state(&state.pool, &rows).await {
        // 不重试：丢失窗口本就允许一个周期；dirty 已清，待下次状态变更再写
        tracing::warn!("Failed to persist dispatch state: {e}");
        return false;
    }
    true
}

/// 启动每 FLUSH_INTERVAL_SECS 秒执行一次 flush 的后台任务。
pub fn spawn_dispatch_flush(state: AppState) {
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(FLUSH_INTERVAL_SECS));
        loop {
            ticker.tick().await;
            flush_once(&state).await;
        }
    });
}
