//! 控制面推送枢纽：各 WebSocket 连接注册发送端，录制线程广播预览帧。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::Mutex;

pub struct PushHub {
    next_id: AtomicU64,
    clients: Mutex<HashMap<u64, SyncSender<String>>>,
}

impl PushHub {
    pub fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            clients: Mutex::new(HashMap::new()),
        }
    }

    pub fn register(&self, tx: SyncSender<String>) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut g) = self.clients.lock() {
            g.insert(id, tx);
        }
        id
    }

    pub fn unregister(&self, id: u64) {
        if let Ok(mut g) = self.clients.lock() {
            g.remove(&id);
        }
    }

    /// 非阻塞广播；队列满或已断开的客户端直接跳过。
    pub fn broadcast(&self, msg: &str) {
        let Ok(g) = self.clients.lock() else {
            return;
        };
        for tx in g.values() {
            let _ = tx.try_send(msg.to_string());
        }
    }
}
