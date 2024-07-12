pub mod utils;

use rand::{
    Rng, SeedableRng,
};
use rand_chacha::ChaCha8Rng;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use utils::custom_map::CustomMap;
use std::hash::Hash;
use std::time::Duration;
use std::fmt::Debug;

pub type Seed = [u8; 32];
pub type Checksum = [u8; 32];

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EventData<S: State> {
    pub event: Event<S>,
    pub seed: Seed,
    pub state_checksum: Checksum,
}

pub type EventIndex = u64;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Req<S: State> {
    Event(S::ClientEvent),
    Sync,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Res<S: State> {
    Sync(SyncData<S>),
    Event(EventData<S>),
    UserUpdate(S::UserId, S::UserData),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SyncData<S: State> {
    pub user_id: S::UserId,
    pub state: StateWrapper<S>,
}

pub trait State: Clone + Debug + Send + Sized + Default + 'static {
    type ServerEvent: ServerEvent<Self>;
    type ClientEvent: ClientEvent;
    type UserId: UserId;
    type UserData: UserData;

    const DURATION_PER_TICK: Duration;

    fn update(&mut self, rng: &mut impl Rng, event: Event<Self>, user_data: Option<&Self::UserData>);
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Event<S: State> {
    ServerEvent(S::ServerEvent),
    ClientEvent(S::ClientEvent, S::UserId),
}

pub trait ServerEvent<S: State>: Clone + Serialize + DeserializeOwned + Send + Debug + Send + 'static {
    fn tick() -> Self;
}

pub trait ClientEvent: Clone + Serialize + DeserializeOwned + Send + Debug + Send + 'static {
    fn init() -> Self;
}

pub trait UserId: Clone + Serialize + DeserializeOwned + Send + Debug + PartialEq + Eq + Hash + Send + 'static {}

pub trait UserData: Clone + Serialize + DeserializeOwned + Send + Debug + Send + 'static {}

pub enum Error {
    InvalidChecksum
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateWrapper<S: State> {
    pub state: S,
    pub users: CustomMap<S::UserId, S::UserData>,
}

impl<S: State> StateWrapper<S> {

    pub fn checksum(&self) -> Checksum
    where
        Self: Serialize
    {
        let serialized = rmp_serde::to_vec(self).unwrap();
        let mut hasher = Sha256::new();
        hasher.update(serialized);
        let slice = &hasher.finalize()[..];
        assert_eq!(slice.len(), 32, "slice length wasn't {}", slice.len());
        slice.try_into().unwrap()
    }

    pub fn update_checked(&mut self, EventData { event, seed, state_checksum }: EventData<S>) -> Result<(), Error>
    where
        Self: Serialize
    {
        let checksum = self.checksum();
        if checksum != state_checksum {
            return Err(Error::InvalidChecksum);
        }

        let mut rng = ChaCha8Rng::from_seed(seed);

        let user_id = if let Event::ClientEvent(_, user_id) = &event {
            Some(user_id.clone())
        } else {
            None
        };

        self.state.update(&mut rng, event, user_id.and_then(|user_id| self.users.get(&user_id)));

        Ok(())
    }
}
