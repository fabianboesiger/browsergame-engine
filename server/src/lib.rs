use rand::{rngs::SmallRng, Rng, SeedableRng};
use engine_shared::{utils::custom_map::CustomMap, Event, EventData, Req, Res, Seed, ServerEvent, State, StateWrapper, SyncData, UserData};
use std::{collections::HashMap, sync::Arc};
use tokio::{
    sync::{broadcast, mpsc, Notify, RwLock},
    time,
};
use serde::Serialize;

struct ServerStateImpl<S: State> {
    state: RwLock<StateWrapper<S>>,
    res_sender: broadcast::Sender<Res<S>>,
    req_sender: mpsc::UnboundedSender<Event<S>>,
}

#[derive(Clone)]
pub struct ServerState<S: State>(Arc<ServerStateImpl<S>>);

#[derive(Debug, Clone)]
pub struct ConnectionReq<S: State> {
    user_id: S::UserId,
    req_sender: mpsc::UnboundedSender<Event<S>>,
    notify: Arc<Notify>,
}

impl<S: State> ConnectionReq<S> {
    pub fn request(&self, req: Req<S>) {
        match req {
            Req::Event(event) => { self.req_sender.send(Event::ClientEvent(event, self.user_id.clone())).ok(); },
            Req::Sync => self.notify.notify_one(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ServerConnectionReq<S: State> {
    res_sender: broadcast::Sender<Res<S>>,
}

impl<S: State> ServerConnectionReq<S> {
    pub fn request(&self, res: Res<S>) {
        self.res_sender.send(res).ok();
    }
}

pub struct ConnectionRes<S: State> {
    user_id: S::UserId,
    state: ServerState<S>,
    notify: Arc<Notify>,
    res_receiver: broadcast::Receiver<Res<S>>,
}

impl<S: State> ConnectionRes<S> {
    pub async fn poll(&mut self) -> Option<Res<S>> {
        tokio::select! {
            _ = self.notify.notified() => {
                let state_wrapper = self.state.0.state.read().await;
                Some(Res::Sync(SyncData {
                    user_id: self.user_id.clone(),
                    state: state_wrapper.clone(),
                }))
            }
            res = self.res_receiver.recv() => {
                match res {
                    Ok(res) => Some(res),
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        let state_wrapper = self.state.0.state.read().await;
                        Some(Res::Sync(SyncData {
                            user_id: self.user_id.clone(),
                            state: state_wrapper.clone(),
                        }))
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        None
                    }
                }
            }
        }
    }
}



#[async_trait::async_trait]
pub trait BackendStore<S: State>: Send + Sync + 'static {
    async fn load_game(&self) -> StateWrapper<S>;
    async fn save_game(&self, state: &StateWrapper<S>);
}

impl<S: State> ServerState<S> {

    pub async fn new<B: BackendStore<S>>(store: B) -> Self
    where
        S: Clone + Serialize,
        RwLock<StateWrapper<S>>: Sync,
    {
        let (req_sender, mut req_receiver) = mpsc::unbounded_channel::<Event<S>>();
        let (res_sender, _res_receiver) = broadcast::channel::<Res<S>>(128);

        let req_sender_clone = req_sender.clone();

        let state = RwLock::new(store.load_game().await);

        let game_state = Arc::new(ServerStateImpl {
            state,
            res_sender,
            req_sender,
        });
        let game_state_clone = game_state.clone();
        let game_state_clone2 = game_state.clone();


        tokio::spawn(async move {
            let mut interval = time::interval(S::DURATION_PER_TICK);

            loop {
                interval.tick().await;

                req_sender_clone
                    .send(Event::ServerEvent(<S::ServerEvent as engine_shared::ServerEvent<S>>::tick()))
                    .ok();

                let state_wrapper = game_state_clone2.state.read().await.clone();
                store.save_game(&state_wrapper).await;
            }
        });

        tokio::spawn(async move {
            let ServerStateImpl {
                state: game,
                res_sender,
                ..
            } = &*game_state_clone;

            let mut rng = SmallRng::from_entropy();

            while let Some(event) = req_receiver.recv().await
            {
                tracing::info!("handling event: {event:?}");


                let mut state = game.write().await;
                let state_checksum = state.checksum();
                let seed: Seed = rng.gen();

                let event = EventData {
                    event,
                    seed,
                    state_checksum,
                };

                if state.update_checked(event.clone()).is_err() {
                    unreachable!();
                }

                tracing::info!("updated state: {state:?}");
                
                res_sender.send(Res::Event(event.clone())).ok();

                
            }
        });

        ServerState(game_state)
    }


   
    pub async fn new_connection(
        &self,
        user_id: S::UserId,
    ) -> (ConnectionReq<S>, ConnectionRes<S>) {
        let notify = Arc::new(Notify::new());
        (ConnectionReq {
            user_id: user_id.clone(),
            req_sender: self.0.req_sender.clone(),
            notify: notify.clone(),
        }, ConnectionRes {
            user_id,
            state: self.clone(),
            res_receiver: self.0.res_sender.subscribe(),
            notify
        })
    }

    pub async fn new_server_connection(
        &self,
    ) -> ServerConnectionReq<S> {
        ServerConnectionReq {
            res_sender: self.0.res_sender.clone(),
        }
    }
}