use std::rc::Rc;

use seed::{prelude::*, *};
use serde::{de::DeserializeOwned, Serialize};
use engine_shared::{
    utils::custom_map::CustomMap, ClientEvent, EventData, GameId, Req, Res, State, SyncData
};

pub struct ClientState<S: State> {
    web_socket: WebSocket,
    web_socket_reconnector: Option<StreamHandle>,
    state: Option<SyncData<S>>,
    ws_path: String,
}

pub trait Msg<S: State>: 'static + From<EventWrapper<S>>  {
    fn send_event(event: S::ClientEvent) -> Self where Self: Sized {
        Self::from(EventWrapper::SendGameEvent(event))
    }
}

impl<S: State> ClientState<S> {
    pub fn init<M: Msg<S>>(orders: &mut impl Orders<M>, ws_path: String) -> Self
    where
        S: DeserializeOwned
    {
        let web_socket = Self::create_websocket(orders, &ws_path);

        ClientState {
            web_socket,
            web_socket_reconnector: None,
            state: None,
            ws_path
        }
    }

    pub fn get_state(&self) -> Option<&S> {
        self.state.as_ref().map(|data| &data.state.state)
    }

    pub fn get_user_id(&self) -> Option<&S::UserId> {
        self.state.as_ref().map(|data| &data.user_id)
    }

    pub fn get_user_data(&self, user_id: &S::UserId) -> Option<&S::UserData> {
        self.state.as_ref().and_then(|data| {
            data.state.users.get(user_id)
        })
    }

    pub fn update<M: Msg<S>>(&mut self, msg: EventWrapper<S>, orders: &mut impl Orders<M>)
    where
        S: DeserializeOwned + Serialize
    {
        let web_socket = &self.web_socket;
        let send = |event| {
            let serialized = rmp_serde::to_vec(&Req::<S>::Event(event)).unwrap();
            web_socket.send_bytes(&serialized).unwrap();
        };

        let sync = || {
            let serialized = rmp_serde::to_vec(&Req::<S>::Sync).unwrap();
            web_socket.send_bytes(&serialized).unwrap();
        };
    
        match msg {
            EventWrapper::WebSocketOpened => {                
                self.web_socket_reconnector = None;
                log!("WebSocket connection is open now");

                sync();
                send(<S::ClientEvent as ClientEvent>::init());
            }
            EventWrapper::CloseWebSocket => {
                self.web_socket_reconnector = None;
                self
                    .web_socket
                    .close(None, Some("user clicked close button"))
                    .unwrap();
            }
            EventWrapper::WebSocketClosed(close_event) => {
                log!(
                    "WebSocket connection was closed, reason:",
                    close_event.reason()
                );
    
                // Chrome doesn't invoke `on_error` when the connection is lost.
                if (!close_event.was_clean() || close_event.code() == 4000) && self.web_socket_reconnector.is_none() {
                    self.web_socket_reconnector = Some(
                        orders.stream_with_handle(streams::backoff(None, EventWrapper::<S>::ReconnectWebSocket)),
                    );
                }
            }
            EventWrapper::WebSocketFailed => {
                log!("WebSocket failed");
                if self.web_socket_reconnector.is_none() {
                    self.web_socket_reconnector = Some(
                        orders.stream_with_handle(streams::backoff(None, EventWrapper::<S>::ReconnectWebSocket)),
                    );
                }
            }
            EventWrapper::ReconnectWebSocket(retries) => {
                log!("Reconnect attempt:", retries);
                self.web_socket = Self::create_websocket(orders, &self.ws_path);
            }
            EventWrapper::SendGameEvent(event) => send(event),    
            EventWrapper::InitGameState(sync_data) => {
                self.state = Some(sync_data);
            }
            EventWrapper::ReceiveGameEvent(event) => {
                if let Some(SyncData { state, .. }) = &mut self.state {
                    if state.update_checked(event).is_err() {
                        log!("invalid state");
                        //web_socket.close(Some(4000), Some("invalid state")).unwrap();
                        sync();
                    }
                }
            }
            EventWrapper::UserUpdate(map) => {
                if let Some(SyncData { state, .. }) = &mut self.state {
                    state.users = map;
                }
            },
        }
    }

    fn create_websocket<M: Msg<S>>(orders: &impl Orders<M>, ws_path: &str) -> WebSocket
    where
        S: DeserializeOwned
    {
        let msg_sender = orders.msg_sender();

        WebSocket::builder(ws_path, orders)
            .on_open(|| M::from(EventWrapper::<S>::WebSocketOpened))
            .on_message(move |msg| Self::decode_message(msg, msg_sender))
            .on_close(|evt| M::from(EventWrapper::<S>::WebSocketClosed(evt)))
            .on_error(|| M::from(EventWrapper::<S>::WebSocketFailed))
            .build_and_open()
            .unwrap()
    }

    fn decode_message<M: Msg<S>>(message: WebSocketMessage, msg_sender: Rc<dyn Fn(Option<M>)>)
    where
        S: DeserializeOwned
    {
        if message.contains_text() {
            unreachable!()
        } else {
            spawn_local(async move {
                let bytes = message
                    .bytes()
                    .await
                    .expect("WebsocketError on binary data");

                let msg: Res<S> = rmp_serde::from_slice(&bytes).unwrap();
                match msg {
                    Res::Event(event) => {
                        msg_sender(Some(M::from(EventWrapper::ReceiveGameEvent(event))));
                    }
                    Res::Sync(sync) => {
                        msg_sender(Some(M::from(EventWrapper::InitGameState(sync))));
                    }
                    Res::UserUpdate(map) => {
                        msg_sender(Some(M::from(EventWrapper::UserUpdate(map))));
                    }
                }
            });
        }
    }
}

#[derive(Debug)]
pub enum EventWrapper<S: State> {
    WebSocketOpened,
    CloseWebSocket,
    WebSocketClosed(CloseEvent),
    WebSocketFailed,
    ReconnectWebSocket(usize),
    SendGameEvent(S::ClientEvent),
    ReceiveGameEvent(EventData<S>),
    InitGameState(SyncData<S>),
    UserUpdate(CustomMap<S::UserId, S::UserData>),
}