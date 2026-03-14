use crate::multiplayer::GameConnection;
use std::{
    any::Any,
    error::Error,
    net::{TcpListener, TcpStream, ToSocketAddrs},
};
use tehuti::{
    engine::{EngineId, EngineMeetingEvent},
    event::{Duplex, Sender},
    protocol::ProtocolFrame,
};
use tehuti_socket::{TcpHost, TcpSession, TcpSessionResult};

pub struct TcpServerConnection {
    host: TcpHost,
    sessions: Vec<TcpSession>,
    events_sender: Option<Sender<EngineMeetingEvent>>,
}

impl Drop for TcpServerConnection {
    fn drop(&mut self) {
        if let Some(events_sender) = &self.events_sender {
            for session in &self.sessions {
                let engine_id = session.remote_engine_id();
                if let Err(error) =
                    events_sender.send(EngineMeetingEvent::UnregisterEngine { engine_id })
                {
                    tracing::event!(
                        target: "quaso::multiplayer::tcp::server",
                        tracing::Level::ERROR,
                        "Failed to send UnregisterEngine event during drop: {}",
                        error
                    );
                }
            }
        }
    }
}

impl TcpServerConnection {
    pub fn new(host: TcpHost) -> Self {
        Self {
            host,
            events_sender: None,
            sessions: Default::default(),
        }
    }

    pub fn make(listener: TcpListener) -> Result<Self, Box<dyn Error>> {
        Ok(Self::new(TcpHost::new(listener, EngineId::uuid())?))
    }

    pub fn listen(address: impl ToSocketAddrs) -> Result<Self, Box<dyn Error>> {
        Self::make(TcpListener::bind(address)?)
    }

    pub fn log_frames(mut self, value: bool) -> Self {
        self.host.log_frames = value;
        for session in &mut self.sessions {
            session.log_frames = value;
        }
        self
    }

    pub fn disconnect(&mut self, engine_id: EngineId) {
        self.sessions
            .retain(|session| session.remote_engine_id() != engine_id);
    }
}

impl GameConnection for TcpServerConnection {
    fn on_register(&mut self, events_sender: &Sender<EngineMeetingEvent>) -> EngineId {
        self.events_sender = Some(events_sender.clone());
        self.host.local_engine_id()
    }

    fn on_unregister(&mut self, events_sender: &Sender<EngineMeetingEvent>) {
        for session in &self.sessions {
            let engine_id = session.remote_engine_id();
            if let Err(error) =
                events_sender.send(EngineMeetingEvent::UnregisterEngine { engine_id })
            {
                tracing::event!(
                    target: "quaso::multiplayer::tcp::server",
                    tracing::Level::ERROR,
                    "Failed to send UnregisterEngine event: {}",
                    error
                );
            }
        }
        self.events_sender = None;
    }

    fn maintain(&mut self) -> bool {
        let mut result = true;
        loop {
            match self.host.accept() {
                Ok(Some(session_result)) => {
                    let TcpSessionResult { session, frames } = session_result;
                    tracing::event!(
                        target: "quaso::multiplayer::tcp::server",
                        tracing::Level::DEBUG,
                        "Accepted new TCP session from {:?} with remote engine ID {:?}",
                        session.peer_addr().unwrap(),
                        session.remote_engine_id()
                    );
                    if let Some(events_sender) = &self.events_sender {
                        let engine_id = session.remote_engine_id();
                        if let Err(error) = events_sender
                            .send(EngineMeetingEvent::RegisterEngine { engine_id, frames })
                        {
                            tracing::event!(
                                target: "quaso::multiplayer::tcp::server",
                                tracing::Level::ERROR,
                                "Failed to send RegisterEngine event: {}",
                                error
                            );
                        }
                        self.sessions.push(session);
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    tracing::event!(
                        target: "quaso::multiplayer::tcp::server",
                        tracing::Level::ERROR,
                        "Failed to accept new TCP session: {}",
                        error
                    );
                    result = false;
                    break;
                }
            }
        }
        self.sessions.retain_mut(|session| {
            if let Err(error) = session.maintain() {
                tracing::event!(
                    target: "quaso::multiplayer::tcp::server",
                    tracing::Level::ERROR,
                    "Failed to maintain session: {}",
                    error
                );
                if let Some(events_sender) = &self.events_sender {
                    let engine_id = session.remote_engine_id();
                    if let Err(error) =
                        events_sender.send(EngineMeetingEvent::UnregisterEngine { engine_id })
                    {
                        tracing::event!(
                            target: "quaso::multiplayer::tcp::server",
                            tracing::Level::ERROR,
                            "Failed to send UnregisterEngine event: {}",
                            error
                        );
                    }
                }
                false
            } else {
                true
            }
        });
        result
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

pub struct TcpClientConnection {
    session: TcpSession,
    frames: Duplex<ProtocolFrame>,
    events_sender: Option<Sender<EngineMeetingEvent>>,
}

impl Drop for TcpClientConnection {
    fn drop(&mut self) {
        if let Some(events_sender) = &self.events_sender {
            let engine_id = self.session.remote_engine_id();
            if let Err(error) =
                events_sender.send(EngineMeetingEvent::UnregisterEngine { engine_id })
            {
                tracing::event!(
                    target: "quaso::multiplayer::tcp::client",
                    tracing::Level::ERROR,
                    "Failed to send UnregisterEngine event during drop: {}",
                    error
                );
            }
        }
    }
}

impl TcpClientConnection {
    pub fn new(session_result: TcpSessionResult) -> Self {
        let TcpSessionResult { session, frames } = session_result;
        tracing::event!(
            target: "quaso::multiplayer::tcp::client",
            tracing::Level::DEBUG,
            "Established TCP session with remote engine ID {} at {:?}",
            session.remote_engine_id(),
            session.peer_addr()
        );
        Self {
            session,
            frames,
            events_sender: None,
        }
    }

    pub fn make(stream: TcpStream) -> Result<Self, Box<dyn Error>> {
        Ok(Self::new(TcpSession::make(stream, EngineId::uuid())?))
    }

    pub fn connect(address: impl ToSocketAddrs) -> Result<Self, Box<dyn Error>> {
        Self::make(TcpStream::connect(address)?)
    }

    pub fn log_frames(mut self, value: bool) -> Self {
        self.session.log_frames = value;
        self
    }
}

impl GameConnection for TcpClientConnection {
    fn on_register(&mut self, events_sender: &Sender<EngineMeetingEvent>) -> EngineId {
        let engine_id = self.session.remote_engine_id();
        tracing::event!(
            target: "quaso::multiplayer::tcp::client",
            tracing::Level::DEBUG,
            "Registering TCP client connection with remote engine ID {} at {:?}",
            engine_id,
            self.session.peer_addr().unwrap()
        );
        if let Err(error) = events_sender.send(EngineMeetingEvent::RegisterEngine {
            engine_id,
            frames: self.frames.clone(),
        }) {
            tracing::event!(
                target: "quaso::multiplayer::tcp::client",
                tracing::Level::ERROR,
                "Failed to send RegisterEngine event: {}",
                error
            );
        }
        self.events_sender = Some(events_sender.clone());
        engine_id
    }

    fn on_unregister(&mut self, events_sender: &Sender<EngineMeetingEvent>) {
        let engine_id = self.session.remote_engine_id();
        tracing::event!(
            target: "quaso::multiplayer::tcp::client",
            tracing::Level::DEBUG,
            "Unregistering TCP client connection with remote engine ID {} at {:?}",
            engine_id,
            self.session.peer_addr().unwrap()
        );
        if let Err(error) = events_sender.send(EngineMeetingEvent::UnregisterEngine { engine_id }) {
            tracing::event!(
                target: "quaso::multiplayer::tcp::client",
                tracing::Level::ERROR,
                "Failed to send UnregisterEngine event: {}",
                error
            );
        }
        self.events_sender = None;
    }

    fn maintain(&mut self) -> bool {
        if let Err(error) = self.session.maintain() {
            tracing::event!(
                target: "quaso::multiplayer::tcp::client",
                tracing::Level::ERROR,
                "Failed to maintain session: {}",
                error
            );
            false
        } else {
            true
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
