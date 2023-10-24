use std::{collections::HashSet, net::SocketAddr, time::Duration};

use coerce::actor::{
    context::ActorContext,
    lifecycle::Stop,
    message::{Handler, Message},
    Actor, ActorId, IntoActorId, ScheduledNotify,
};
use tracing::{error, info, instrument};

use crate::{
    container_connection::Connection,
    container_mgmt::{new_container, DeployedContainer},
    create_connection::ClientConnection,
    OPTS,
};

pub struct Container {
    deployment: Option<DeployedContainer>,
    connections: HashSet<ActorId>,
    stop_cancel: Option<ScheduledNotify<Self, Stop>>,
}

impl Container {
    pub fn new() -> Self {
        Self {
            deployment: None,
            connections: HashSet::new(),
            stop_cancel: None,
        }
    }
}

#[async_trait::async_trait]
impl Actor for Container {
    #[instrument(skip_all, fields(path = %ctx.full_path()))]
    async fn started(&mut self, ctx: &mut ActorContext) {
        let deployment = match new_container().await {
            Ok(deployment) => deployment,
            Err(err) => {
                error!("Failed to start container: {:?}", err);
                ctx.stop(None);
                return;
            }
        };

        info!(id = %deployment.id, "Container started");

        self.deployment = Some(deployment)
    }

    #[instrument(skip_all, fields(path = %ctx.full_path()))]
    async fn on_child_stopped(&mut self, id: &ActorId, ctx: &mut ActorContext) {
        self.connections.remove(id);
        info!(
            num_connections = self.connections.len(),
            "Connection closed"
        );

        if self.connections.is_empty() {
            info!("All connections exhausted for {}, scheduling stop", id);
            if self.stop_cancel.is_none() {
                self.stop_cancel = Some(
                    self.actor_ref(ctx)
                        .scheduled_notify(Stop(None), Duration::from_secs(OPTS.timeout as u64)),
                );
            }
        }
    }

    #[instrument(skip_all, fields(path = %_ctx.full_path()))]
    async fn stopped(&mut self, _ctx: &mut ActorContext) {
        if let Some(deployment) = self.deployment.as_mut() {
            info!("Stopping container {}", deployment.id);

            deployment.stop().await;
        }
    }
}

pub struct Connect {
    pub client_addr: SocketAddr,
    pub port: u16,
    pub connection: ClientConnection,
}

impl Message for Connect {
    type Result = ();
}

#[async_trait::async_trait]
impl Handler<Connect> for Container {
    #[instrument(skip_all, fields(path = %ctx.full_path()))]
    async fn handle(&mut self, message: Connect, ctx: &mut ActorContext) {
        if let Some(stop) = self.stop_cancel.take() {
            stop.cancel();
        }

        let Some(deployment) = self.deployment.as_ref() else {
            info!("Deployment wasn't ready, quitting");
            ctx.stop(None);
            return;
        };

        if !deployment.check_up().await {
            // if we're dead, just drop everything
            info!("Deployment check up failed, quitting");
            ctx.stop(None);
            return;
        }

        info!(
            "Creating connection from {} to {}",
            message.client_addr, message.port
        );

        match message
            .connection
            .create_connection(message.port, deployment)
            .await
        {
            Ok(container_conn) => {
                let actor = ctx
                    .spawn(
                        format!(
                            "connection:{}->{}:{}",
                            message.client_addr, deployment.id, message.port
                        )
                        .into_actor_id(),
                        Connection::new(message.connection.into_connection(), container_conn),
                    )
                    .await
                    .unwrap();

                self.connections.insert(actor.id.clone());

                info!(
                    num_connections = self.connections.len(),
                    "Connection created"
                )
            }
            Err(e) => {
                error!("Failed creating connection: {:?}", e);
            }
        }
    }
}
