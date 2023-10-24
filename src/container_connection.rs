use abort_on_drop::ChildTask;
use coerce::actor::{context::ActorContext, Actor};
use tokio::io::{AsyncRead, AsyncWrite};
use tracing::{debug, info, instrument, Instrument};

pub struct Connection {
    mux_task: Option<ChildTask<()>>,
    notif_task: Option<ChildTask<()>>,
}

impl Connection {
    pub fn new<LS, RS>(lhs: LS, rhs: RS) -> Self
    where
        LS: AsyncRead + AsyncWrite + Send + Unpin + 'static,
        RS: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    {
        let mux_task =
            tokio::spawn(rx_tx_loop(lhs, rhs).instrument(tracing::Span::current())).into();

        Self {
            mux_task: Some(mux_task),
            notif_task: None,
        }
    }
}

#[async_trait::async_trait]
impl Actor for Connection {
    #[instrument(skip_all, fields(path = %ctx.full_path()))]
    async fn started(&mut self, ctx: &mut ActorContext) {
        info!("Started connection");
        self.notif_task = Some(
            tokio::spawn({
                let actor_ref = self.actor_ref(ctx);
                let task_handle = self.mux_task.take().unwrap();

                async move {
                    let _ = task_handle.await;
                    let _ = actor_ref.stop().await;
                }
            })
            .into(),
        )
    }
}

async fn rx_tx_loop<LS, RS>(mut lhs: LS, mut rhs: RS)
where
    LS: AsyncRead + AsyncWrite + Send + Unpin,
    RS: AsyncRead + AsyncWrite + Send + Unpin,
{
    info!("Starting tx-rx loop");

    let _ = tokio::io::copy_bidirectional(&mut lhs, &mut rhs).await;
    debug!("Stopping transmission for");
}
