use rand::{rngs::SmallRng, Rng, SeedableRng};
use engine_shared::{utils::custom_map::CustomMap, Event, EventData, Req, Res, Seed, State, StateWrapper, SyncData};
use std::sync::Arc;
use tokio::{
    sync::{broadcast, mpsc, Notify, RwLock},
    time,
};
use serde::Serialize;

struct ServerStateImpl<S: State> {
    state: RwLock<StateWrapper<S>>,
    res_sender: broadcast::Sender<Res<S>>,
    req_sender: mpsc::UnboundedSender<Event<S>>,
    update_user_data: Arc<Notify>, 
    updated_user_data: Arc<Notify>, 
}

#[derive(Clone)]
pub struct ServerState<S: State>(Arc<ServerStateImpl<S>>);

#[derive(Debug, Clone)]
pub struct ClientConnectionReq<S: State> {
    user_id: S::UserId,
    req_sender: mpsc::UnboundedSender<Event<S>>,
    sync_state: Arc<Notify>,
}

impl<S: State> ClientConnectionReq<S> {
    pub fn request(&self, req: Req<S>) {
        match req {
            Req::Event(event) => { self.req_sender.send(Event::ClientEvent(event, self.user_id.clone())).ok(); },
            Req::Sync => self.sync_state.notify_one(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ServerConnectionReq<S: State> {
    update_user_data: Arc<Notify>,
    _phantom: std::marker::PhantomData<S>,
}

impl<S: State> ServerConnectionReq<S> {
    pub fn updated_user_data(&self) {
        self.update_user_data.notify_one();
    }
}

pub struct ClientConnectionRes<S: State> {
    user_id: S::UserId,
    state: ServerState<S>,
    sync_state: Arc<Notify>,
    updated_user_data: Arc<Notify>,
    res_receiver: broadcast::Receiver<Res<S>>,
}

impl<S: State> ClientConnectionRes<S> {
    pub async fn poll(&mut self) -> Option<Res<S>> {
        tokio::select! {
            _ = self.sync_state.notified() => {
                let state_wrapper = self.state.0.state.read().await;
                Some(Res::Sync(SyncData {
                    user_id: self.user_id.clone(),
                    state: state_wrapper.clone(),
                }))
            }
            _ = self.updated_user_data.notified() => {
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
    async fn load_game(&self) -> S;
    async fn save_game(&self, state: &S);
    async fn load_user_data(&self) -> CustomMap<S::UserId, S::UserData>;
}

impl<S: State> ServerState<S> {

    pub async fn new<B: BackendStore<S>>(store: B) -> Self
    where
        S: Clone + Serialize,
        RwLock<StateWrapper<S>>: Sync,
    {
        let store = Arc::new(store);
        let (req_sender, mut req_receiver) = mpsc::unbounded_channel::<Event<S>>();
        let (res_sender, _res_receiver) = broadcast::channel::<Res<S>>(128);

        let req_sender_clone = req_sender.clone();

        let state = store.load_game().await;
        let user_data = store.load_user_data().await;
        let state = RwLock::new(StateWrapper {
            state,
            users: CustomMap::from(user_data),
        });

        let update_user_data = Arc::new(Notify::new());
        let updated_user_data = Arc::new(Notify::new());

        let game_state = Arc::new(ServerStateImpl {
            state,
            res_sender,
            req_sender,
            update_user_data,
            updated_user_data
        });

        let game_state_clone = game_state.clone();
        let store_clone = store.clone();
        tokio::spawn(async move {
            let mut interval = time::interval(S::DURATION_PER_TICK);

            loop {
                interval.tick().await;

                req_sender_clone
                    .send(Event::ServerEvent(<S::ServerEvent as engine_shared::ServerEvent<S>>::tick()))
                    .ok();

                let state_wrapper = game_state_clone.state.read().await.clone();
                store_clone.save_game(&state_wrapper.state).await;
            }
        });

        let game_state_clone = game_state.clone();
        let store_clone = store.clone();
        tokio::spawn(async move {
            let ServerStateImpl {
                state: game,
                update_user_data,
                updated_user_data,
                ..
            } = &*game_state_clone;

            let update_user_data_clone = update_user_data.clone();
            let updated_user_data_clone = updated_user_data.clone();

            loop {
                update_user_data_clone.notified().await;
                game.write().await.users = store_clone.load_user_data().await;
                updated_user_data_clone.notify_waiters();
            }
        });

        let game_state_clone = game_state.clone();
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
    ) -> (ClientConnectionReq<S>, ClientConnectionRes<S>) {
        let sync_state = Arc::new(Notify::new());
        (ClientConnectionReq {
            user_id: user_id.clone(),
            req_sender: self.0.req_sender.clone(),
            sync_state: sync_state.clone(),
        }, ClientConnectionRes {
            user_id,
            state: self.clone(),
            res_receiver: self.0.res_sender.subscribe(),
            sync_state,
            updated_user_data: self.0.updated_user_data.clone(),
        })
    }

    pub async fn new_server_connection(
        &self,
    ) -> ServerConnectionReq<S> {
        ServerConnectionReq {
            update_user_data: self.0.update_user_data.clone(),
            _phantom: std::marker::PhantomData,
        }
    }
}