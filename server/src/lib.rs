use rand::{rngs::SmallRng, Rng, SeedableRng};
use engine_shared::{utils::custom_map::CustomMap, Event, EventData, GameId, Req, Res, Seed, State, StateWrapper, SyncData};
use std::{collections::HashMap, sync::{atomic::AtomicBool, Arc}};
use tokio::{
    sync::{broadcast, mpsc, Notify, RwLock},
    time,
};
use serde::Serialize;

pub type GameVersion = i64;

struct ServerStateImpl<S: State> {
    state: RwLock<StateWrapper<S>>,
    res_sender: broadcast::Sender<Res<S>>,
    req_sender: mpsc::UnboundedSender<Event<S>>,
}

#[derive(Clone)]
pub struct ServerState<S: State> {
    update_user_data: Arc<Notify>, 
    updated_user_data: Arc<Notify>,
    games: HashMap<GameId, Arc<ServerStateImpl<S>>>
}

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
    game_id: GameId,
    state: ServerState<S>,
    sync_state: Arc<Notify>,
    updated_user_data: Arc<Notify>,
    res_receiver: broadcast::Receiver<Res<S>>,
}

impl<S: State> ClientConnectionRes<S> {
    pub async fn poll(&mut self) -> Option<Res<S>> {
        let state = &self.state.games.get(&self.game_id).unwrap().state;

        tokio::select! {
            _ = self.sync_state.notified() => {
                let state_wrapper = state.read().await;
                Some(Res::Sync(SyncData {
                    user_id: self.user_id.clone(),
                    state: state_wrapper.clone(),
                }))
            }
            _ = self.updated_user_data.notified() => {
                let state_wrapper = state.read().await;
                Some(Res::Sync(SyncData {
                    user_id: self.user_id.clone(),
                    state: state_wrapper.clone(),
                }))
            }
            res = self.res_receiver.recv() => {
                match res {
                    Ok(res) => Some(res),
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        let state_wrapper = state.read().await;
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
    async fn load_game(&self, game_id: GameId) -> S;
    async fn save_game(&self, game_id: GameId, state: &S);
    async fn load_user_data(&self) -> CustomMap<S::UserId, S::UserData>;
}

impl<S: State> ServerState<S> {

    pub fn new() -> Self {
        ServerState {
            games: HashMap::new(),
            update_user_data: Arc::new(Notify::new()),
            updated_user_data: Arc::new(Notify::new()),
        }
    }

    pub async fn add<B: BackendStore<S>>(&mut self, store: B, game_id: GameId)
    where
        S: Clone + Serialize,
        RwLock<StateWrapper<S>>: Sync,
    {
        let world_closed = Arc::new(Notify::new());

        let store = Arc::new(store);
        let (req_sender, mut req_receiver) = mpsc::unbounded_channel::<Event<S>>();
        let (res_sender, _res_receiver) = broadcast::channel::<Res<S>>(128);

        let req_sender_clone = req_sender.clone();

        let state = store.load_game(game_id).await;
        let user_data = store.load_user_data().await;
        let state = RwLock::new(StateWrapper {
            state,
            users: CustomMap::from(user_data),
        });

        let game_state = Arc::new(ServerStateImpl {
            state,
            res_sender,
            req_sender,
        });

        let game_state_clone = game_state.clone();
        let store_clone = store.clone();
        let join_handle_tick = tokio::spawn(async move {
            let mut interval = time::interval(S::DURATION_PER_TICK);

            loop {
                interval.tick().await;

                req_sender_clone
                    .send(Event::ServerEvent(<S::ServerEvent as engine_shared::ServerEvent<S>>::tick()))
                    .ok();

                let state_wrapper = game_state_clone.state.read().await.clone();
                store_clone.save_game(game_id, &state_wrapper.state).await;
            }
        });

        let game_state_clone = game_state.clone();
        let store_clone = store.clone();
        let update_user_data = self.update_user_data.clone();
        let updated_user_data = self.updated_user_data.clone();
        let join_handle_update_user_data = tokio::spawn(async move {
            let update_user_data_clone = update_user_data.clone();
            let updated_user_data_clone = updated_user_data.clone();

            loop {
                update_user_data_clone.notified().await;
                game_state_clone.state.write().await.users = store_clone.load_user_data().await;
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
                tracing::debug!("handling event: {event:?}");


                let mut state_wrapper = game.write().await;
                let state_checksum = state_wrapper.checksum();
                let seed: Seed = rng.gen();

                let event = EventData {
                    event,
                    seed,
                    state_checksum,
                };

                match state_wrapper.update_checked(event.clone()) {
                    Ok(()) => {},
                    Err(engine_shared::Error::WorldClosed) => {
                        join_handle_tick.abort();
                        join_handle_update_user_data.abort();

                        break;
                    },
                    Err(_) => panic!()
                }

                tracing::debug!("updated state: {state_wrapper:?}");
                
                res_sender.send(Res::Event(event.clone())).ok(); 
            }
        });

      

        self.games.insert(game_id, game_state);
    }


   
    pub async fn new_connection(
        &self,
        user_id: S::UserId,
        game_id: GameId,
    ) -> (ClientConnectionReq<S>, ClientConnectionRes<S>) {
        let sync_state = Arc::new(Notify::new());
        let game = self.games.get(&game_id).unwrap();
        (ClientConnectionReq {
            user_id: user_id.clone(),
            req_sender: game.req_sender.clone(),
            sync_state: sync_state.clone(),
        }, ClientConnectionRes {
            user_id,
            state: self.clone(),
            res_receiver: game.res_sender.subscribe(),
            sync_state,
            updated_user_data: self.updated_user_data.clone(),
            game_id,
        })
    }

    pub async fn new_server_connection(
        &self,
    ) -> ServerConnectionReq<S> {
        ServerConnectionReq {
            update_user_data: self.update_user_data.clone(),
            _phantom: std::marker::PhantomData,
        }
    }
}