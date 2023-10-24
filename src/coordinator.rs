use std::net::{IpAddr, SocketAddr};

use coerce::actor::{
    context::ActorContext,
    message::{Handler, Message},
    Actor, ActorId, IntoActorId, LocalActorRef,
};
use double_map::DHashMap;
use tracing::instrument;

use crate::{
    container::{self, Container},
    create_connection::ClientConnection,
};

pub struct Coordinator {
    containers: DHashMap<IpAddr, ActorId, LocalActorRef<Container>>,
}

impl Coordinator {
    pub fn new() -> Self {
        Self {
            containers: DHashMap::new(),
        }
    }
}

#[async_trait::async_trait]
impl Actor for Coordinator {
    async fn on_child_stopped(&mut self, id: &ActorId, _ctx: &mut ActorContext) {
        let _ = self.containers.remove_key2(id);
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
impl Handler<Connect> for Coordinator {
    #[instrument(skip_all, fields(path = %ctx.full_path()))]
    async fn handle(&mut self, message: Connect, ctx: &mut ActorContext) {
        let container = if let Some(container) = self.containers.get_key1(&message.client_addr.ip())
        {
            container
        } else {
            let container = ctx
                .spawn_deferred(
                    format!("container:{}", message.client_addr.ip()).into_actor_id(),
                    Container::new(),
                )
                .unwrap();
            self.containers
                .entry(message.client_addr.ip(), container.id.clone())
                .unwrap()
                .or_insert(container)
        };

        container
            .notify(container::Connect {
                client_addr: message.client_addr,
                port: message.port,
                connection: message.connection,
            })
            .unwrap();
    }
}
