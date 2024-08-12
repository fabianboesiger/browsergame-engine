use engine_shared::{
    utils::custom_map::CustomMap,
    Event, EventData, GameId, Req, Res, Seed, State, StateWrapper, SyncData,
};
use rand::{rngs::SmallRng, Rng, SeedableRng};
use serde::Serialize;
use std::{collections::HashMap, sync::Arc};
use tokio::{
    sync::{broadcast, mpsc, Notify, RwLock},
    task::JoinHandle,
    time,
};

pub type GameVersion = i64;

#[derive(Debug, Clone, Copy)]
pub enum Error {
    GameNotFound
}

impl std::error::Error for Error {}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "game not found")
    }
}

struct ServerStateImpl<S: State> {
    state: RwLock<StateWrapper<S>>,
    res_sender: broadcast::Sender<Res<S>>,
    req_sender: mpsc::UnboundedSender<Event<S>>,
}

pub struct ServerState<S: State, B: BackendStore<S>> {
    update_user_data: Arc<Notify>,
    updated_user_data: Arc<Notify>,
    games: Arc<RwLock<HashMap<GameId, Arc<ServerStateImpl<S>>>>>,
    store: Arc<B>,
}

impl<S: State, B: BackendStore<S>> Clone for ServerState<S, B> {
    fn clone(&self) -> Self {
        ServerState {
            update_user_data: self.update_user_data.clone(),
            updated_user_data: self.updated_user_data.clone(),
            games: self.games.clone(),
            store: self.store.clone(),
        }
    }
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
            Req::Event(event) => {
                self.req_sender
                    .send(Event::ClientEvent(event, self.user_id.clone()))
                    .ok();
            }
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

pub struct ClientConnectionRes<S: State, B: BackendStore<S>> {
    user_id: S::UserId,
    game_id: GameId,
    state: ServerState<S, B>,
    sync_state: Arc<Notify>,
    updated_user_data: Arc<Notify>,
    res_receiver: broadcast::Receiver<Res<S>>,
}

impl<S: State, B: BackendStore<S>> ClientConnectionRes<S, B> {
    pub async fn poll(&mut self) -> Option<Res<S>> {
        let games = self.state.games.read().await;
        let state = &games.get(&self.game_id).unwrap().state;

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
                Some(Res::UserUpdate(state_wrapper.users.clone()))
            }
            res = self.res_receiver.recv() => {
                match res {
                    Ok(res) => Some(res),
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        // If receiver lagged, retransmit the whole state.
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
    type Error: std::error::Error;

    async fn create_game(&self) -> Result<GameId, Self::Error>;
    async fn load_game(&self, game_id: GameId) -> Result<S, Self::Error>;
    async fn save_game(&self, game_id: GameId, state: &S) -> Result<(), Self::Error>;
    async fn load_user_data(&self) -> Result<CustomMap<S::UserId, S::UserData>, Self::Error>;
}

impl<S: State, B: BackendStore<S>> ServerState<S, B> {
    pub fn new(store: B) -> Self {
        ServerState {
            games: Arc::new(RwLock::new(HashMap::new())),
            update_user_data: Arc::new(Notify::new()),
            updated_user_data: Arc::new(Notify::new()),
            store: Arc::new(store),
        }
    }

    pub async fn read_games<F>(&self, mut f: F)
    where
        F: FnMut(&S),
    {
        for game in self.games.read().await.values() {
            let state = &game.state.read().await.state;
            f(state)
        }
    }

    pub async fn create(&self) -> Result<(), B::Error>
    where
        S: Clone + Serialize,
        RwLock<StateWrapper<S>>: Sync,
        B::Error: Send,
    {
        let game_id = self.store.create_game().await?;
        self.load(game_id).await?;

        Ok(())
    }

    pub async fn load(&self, game_id: GameId) -> Result<(), B::Error>
    where
        S: Clone + Serialize,
        RwLock<StateWrapper<S>>: Sync,
        B::Error: Send,
    {
        let (set_state_to_save, mut get_state_to_save) = mpsc::channel::<S>(1);

        let (req_sender, mut req_receiver) = mpsc::unbounded_channel::<Event<S>>();
        let (res_sender, _res_receiver) = broadcast::channel::<Res<S>>(128);

        let req_sender_clone = req_sender.clone();

        let state = self.store.load_game(game_id).await?;
        let user_data = self.store.load_user_data().await?;
        let state = RwLock::new(StateWrapper {
            state,
            users: CustomMap::from(user_data),
        });

        let game_state = Arc::new(ServerStateImpl {
            state,
            res_sender,
            req_sender,
        });

        let join_handle_tick = tokio::spawn(async move {
            let mut interval = time::interval(S::DURATION_PER_TICK);

            loop {
                interval.tick().await;

                req_sender_clone
                    .send(Event::ServerEvent(
                        <S::ServerEvent as engine_shared::ServerEvent<S>>::tick(),
                    ))
                    .ok();
            }
        });

        let game_state_clone = game_state.clone();
        let store_clone = self.store.clone();
        let update_user_data = self.update_user_data.clone();
        let updated_user_data = self.updated_user_data.clone();
        let join_handle_update_user_data: JoinHandle<Result<(), B::Error>> =
            tokio::spawn(async move {
                let update_user_data_clone = update_user_data.clone();
                let updated_user_data_clone = updated_user_data.clone();

                loop {
                    update_user_data_clone.notified().await;
                    game_state_clone.state.write().await.users =
                        store_clone.load_user_data().await?;
                    updated_user_data_clone.notify_waiters();
                }
            });

        let game_state_clone = game_state.clone();
        let join_handle_events = tokio::spawn(async move {
            let ServerStateImpl {
                state: game,
                res_sender,
                ..
            } = &*game_state_clone;

            let mut rng = SmallRng::from_entropy();

            while let Some(event) = req_receiver.recv().await {
                tracing::debug!("handling event: {event:?}");

                let mut state_wrapper = game.write().await;
                let state_checksum = state_wrapper.checksum();
                let seed: Seed = rng.gen();

                let event = EventData {
                    event,
                    seed,
                    state_checksum,
                };

                let res = state_wrapper.update_checked(event.clone());
                match res {
                    Ok(()) => {
                        set_state_to_save.try_send(state_wrapper.state.clone()).ok();
                    }
                    Err(engine_shared::Error::WorldClosed) => {
                        set_state_to_save.try_send(state_wrapper.state.clone()).ok();
                    }
                    Err(_) => panic!(),
                }

                tracing::debug!("updated state: {state_wrapper:?}");

                res_sender.send(Res::Event(event.clone())).ok();
            }
        });

        let store_clone = self.store.clone();
        let games = self.games.clone();
        let _: JoinHandle<Result<(), B::Error>> = tokio::spawn(async move {
            while let Some(state) = get_state_to_save.recv().await {
                store_clone.save_game(game_id, &state).await?;
                if let Some(winner) = state.has_winner() {
                    tracing::info!("the world {} was closed, winner is {:?}", game_id, winner);
                    break;
                }
            }

            join_handle_tick.abort();
            join_handle_update_user_data.abort();
            join_handle_events.abort();

            games.write().await.remove(&game_id);

            Ok(())
        });

        self.games.write().await.insert(game_id, game_state);

        Ok(())
    }

    pub async fn new_connection(
        &self,
        user_id: S::UserId,
        game_id: GameId,
    ) -> Result<(ClientConnectionReq<S>, ClientConnectionRes<S, B>), Error> {
        let sync_state = Arc::new(Notify::new());
        let games = self.games.read().await;
        let game = games.get(&game_id).ok_or(Error::GameNotFound)?;
        Ok((
            ClientConnectionReq {
                user_id: user_id.clone(),
                req_sender: game.req_sender.clone(),
                sync_state: sync_state.clone(),
            },
            ClientConnectionRes {
                user_id,
                state: self.clone(),
                res_receiver: game.res_sender.subscribe(),
                sync_state,
                updated_user_data: self.updated_user_data.clone(),
                game_id,
            },
        ))
    }

    pub async fn new_server_connection(&self) -> ServerConnectionReq<S> {
        ServerConnectionReq {
            update_user_data: self.update_user_data.clone(),
            _phantom: std::marker::PhantomData,
        }
    }
}
