use std::sync::Arc;

use super::OBC;
use crate::event::Event;
use crate::util::ProtocolItem;
use crate::{ActionHandler, EventHandler, OneBot};
use crate::{GetStatus, WalleResult};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

#[cfg(feature = "http")]
mod impl_http;
#[cfg(feature = "websocket")]
mod impl_ws;

/// OneBotConnect 实现端实现
///
/// ImplOBC impl EventHandler 接收 Event 并外发处理
///
/// ImplOBC 仅对 Event 泛型要求 Clone trait
pub struct ImplOBC<E> {
    pub implt: String,
    pub(crate) event_tx: tokio::sync::broadcast::Sender<E>,
    pub(crate) hb_tx: tokio::sync::broadcast::Sender<crate::event::Event>,
}

impl<E, A, R> EventHandler<E, A, R> for ImplOBC<E>
where
    E: ProtocolItem + Clone,
    A: ProtocolItem,
    R: ProtocolItem,
{
    type Config = crate::config::ImplConfig;
    async fn start<AH, EH>(
        &self,
        ob: &Arc<OneBot<AH, EH>>,
        config: crate::config::ImplConfig,
    ) -> WalleResult<Vec<JoinHandle<()>>>
    where
        AH: ActionHandler<E, A, R> + Send + Sync + 'static,
        EH: EventHandler<E, A, R> + Send + Sync + 'static,
    {
        let mut tasks = vec![];
        #[cfg(feature = "websocket")]
        {
            self.ws(ob, config.websocket, &mut tasks).await?;
            self.wsr(ob, config.websocket_rev, &mut tasks).await?;
        }
        #[cfg(feature = "http")]
        {
            self.http(ob, config.http, &mut tasks).await?;
            self.webhook(ob, config.http_webhook, &mut tasks).await?;
        }
        if config.heartbeat.enabled {
            tasks.push(start_hb(ob, config.heartbeat.interval, self.hb_tx.clone()))
        }
        Ok(tasks)
    }
    async fn call<AH, EH>(&self, event: E, _ob: &Arc<OneBot<AH, EH>>) -> WalleResult<()>
    where
        AH: ActionHandler<E, A, R> + Send + Sync + 'static,
        EH: EventHandler<E, A, R> + Send + Sync + 'static,
    {
        self.event_tx.send(event).ok();
        Ok(())
    }
}

impl<E> ImplOBC<E> {
    pub fn new(implt: String) -> Self
    where
        E: Clone,
    {
        let (event_tx, _) = tokio::sync::broadcast::channel(1024); //todo
        let (hb_tx, _) = tokio::sync::broadcast::channel(1024);
        Self {
            implt,
            event_tx,
            hb_tx,
        }
    }
}

async fn build_hb<AH, EH>(ob: &OneBot<AH, EH>, interval: u32) -> crate::event::Event
where
    AH: GetStatus + Send + Sync,
{
    let status = ob.action_handler.get_status().await;
    crate::event::Event {
        id: crate::util::new_uuid(),
        time: crate::util::timestamp_nano_f64(),
        ty: "meta".to_string(),
        detail_type: "heartbeat".to_string(),
        sub_type: "".to_string(),
        extra: crate::value_map! {
            "interval": interval,
            "status": status
        },
    }
}

fn start_hb<AH, EH>(
    ob: &Arc<OneBot<AH, EH>>,
    interval: u32,
    hb_tx: broadcast::Sender<Event>,
) -> JoinHandle<()>
where
    AH: GetStatus + Send + Sync + 'static,
    EH: Send + Sync + 'static,
{
    let hb_tx = Arc::new(hb_tx);
    let mut signal = ob.get_signal_rx().unwrap();
    let ob = ob.clone();
    tokio::spawn(async move {
        loop {
            if signal.try_recv().is_ok() {
                break;
            }
            hb_tx.send(build_hb(&ob, interval).await).ok();
            tokio::time::sleep(std::time::Duration::from_secs(interval as u64)).await;
        }
    })
}
