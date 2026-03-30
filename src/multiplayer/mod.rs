pub mod client_server;
pub mod clock;
pub mod csp_ssr;
pub mod ggpo;
pub mod local;
pub mod rollback;
pub mod tcp;
pub mod universal;

use crate::{context::GameContext, game::GameState};
use std::{
    any::Any,
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use tehuti::{
    engine::{
        EngineId, EngineMeeting, EngineMeetingConfig, EngineMeetingEvent, EngineMeetingResult,
    },
    event::Sender,
    meeting::MeetingInterface,
    peer::{PeerFactory, PeerId},
};
use tehuti_timeline::time::TimeStamp;

pub trait GameConnection {
    fn on_register(&mut self, events_sender: &Sender<EngineMeetingEvent>) -> EngineId;

    fn on_unregister(&mut self, events_sender: &Sender<EngineMeetingEvent>);

    fn maintain(&mut self) -> bool;

    fn as_any(&self) -> &dyn Any;

    fn as_any_mut(&mut self) -> &mut dyn Any;
}

static MEETING_INDEX: AtomicUsize = AtomicUsize::new(0);

pub struct GameNetwork {
    meeting: EngineMeeting,
    pub interface: MeetingInterface,
    events_sender: Sender<EngineMeetingEvent>,
    connections: HashMap<EngineId, Box<dyn GameConnection>>,
}

impl Drop for GameNetwork {
    fn drop(&mut self) {
        self.clear_connections();
        if let Err(error) = self.meeting.maintain() {
            tracing::event!(
                target: "quaso::multiplayer::network",
                tracing::Level::ERROR,
                "Failed to maintain meeting on drop: {}",
                error
            );
        }
    }
}

impl Default for GameNetwork {
    fn default() -> Self {
        Self::new(Default::default())
    }
}

impl GameNetwork {
    pub fn new(factory: Arc<PeerFactory>) -> Self {
        let index = MEETING_INDEX.fetch_add(1, Ordering::SeqCst);
        let EngineMeetingResult {
            meeting,
            interface,
            events_sender,
        } = EngineMeeting::make(index, Default::default(), factory);
        Self {
            meeting,
            interface,
            events_sender,
            connections: Default::default(),
        }
    }

    pub fn with_engine_config(mut self, config: EngineMeetingConfig) -> Self {
        self.meeting.config = config;
        self
    }

    pub fn engine_config(&self) -> &EngineMeetingConfig {
        &self.meeting.config
    }

    pub fn engine_config_mut(&mut self) -> &mut EngineMeetingConfig {
        &mut self.meeting.config
    }

    pub fn engine_meeting_factory(&self) -> &Arc<PeerFactory> {
        self.meeting.meeting_factory()
    }

    pub fn engine_peers(&self) -> impl Iterator<Item = PeerId> {
        self.meeting.peers()
    }

    pub fn engine_meeting_peers(&self) -> impl Iterator<Item = PeerId> {
        self.meeting.meeting_peers()
    }

    pub fn engines(&self) -> impl Iterator<Item = EngineId> {
        self.meeting.engines()
    }

    pub fn add_connection(&mut self, mut connection: impl GameConnection + 'static) -> EngineId {
        let engine_id = connection.on_register(&self.events_sender);
        self.connections.insert(engine_id, Box::new(connection));
        engine_id
    }

    pub fn remove_connection(&mut self, engine_id: EngineId) {
        if let Some(mut connection) = self.connections.remove(&engine_id) {
            connection.on_unregister(&self.events_sender);
        }
    }

    pub fn clear_connections(&mut self) {
        for connection in self.connections.values_mut() {
            connection.on_unregister(&self.events_sender);
        }
        self.connections.clear();
    }

    pub fn connection(&self, engine_id: EngineId) -> Option<&dyn GameConnection> {
        self.connections.get(&engine_id).map(|c| c.as_ref())
    }

    pub fn connection_mut(&mut self, engine_id: EngineId) -> Option<&mut dyn GameConnection> {
        match self.connections.get_mut(&engine_id) {
            Some(connection) => Some(connection.as_mut()),
            None => None,
        }
    }

    pub fn maintain(&mut self) {
        self.connections.retain(|id, connection| {
            if !connection.maintain() {
                tracing::event!(
                    target: "quaso::multiplayer::network",
                    tracing::Level::WARN,
                    "Connection {:?} failed to maintain and will be removed",
                    id
                );
                connection.on_unregister(&self.events_sender);
                false
            } else {
                true
            }
        });
        if let Err(error) = self.meeting.maintain() {
            tracing::event!(
                target: "quaso::multiplayer::network",
                tracing::Level::ERROR,
                "Failed to maintain meeting: {}",
                error
            );
        }
    }
}

#[derive(Default)]
pub enum GameMultiplayerChange {
    #[default]
    None,
    Set(Box<dyn GameMultiplayer>),
    Reset,
}

pub trait GameMultiplayer {
    fn current_tick(&self) -> TimeStamp;

    #[allow(unused_variables)]
    fn on_startup(&mut self, state: &mut dyn GameState, context: GameContext) {}

    #[allow(unused_variables)]
    fn on_cleanup(&mut self, state: &mut dyn GameState, context: GameContext) {}

    fn maintain(&mut self, state: &mut dyn GameState, context: GameContext, delta_time: f32);

    fn as_any(&self) -> &dyn Any;

    fn as_any_mut(&mut self) -> &mut dyn Any;
}

#[macro_export]
macro_rules! inputs_bitstruct {
    (
        $(#[$struct_meta:meta])*
        $struct_vis:vis struct $struct_name:ident ($inner:ty) {
            $(
                $(#[$field_meta:meta])*
                $field_name:ident : $field_offset:literal
            ),* $(,)?
        }
    ) => {
        $(#[$struct_meta])*
        $struct_vis struct $struct_name($inner);

        impl $struct_name {
            #[allow(dead_code)]
            pub fn new(inner: $inner) -> Self {
                Self(inner)
            }

            #[allow(dead_code)]
            pub fn inner(&self) -> $inner {
                self.0
            }

            #[allow(dead_code)]
            pub fn none() -> Self {
                Self::new(0)
            }

            #[allow(dead_code)]
            pub fn all() -> Self {
                let mut value: $inner = 0;
                $(
                    value |= 1 << $field_offset;
                )*
                Self::new(value)
            }

            $(
                $crate::third_party::paste::paste! {
                    #[allow(non_upper_case_globals, dead_code)]
                    pub const [<FIELD_ $field_name>]: $inner = $field_offset;
                }

                #[allow(dead_code)]
                pub fn $field_name(&self) -> bool {
                    (self.0 & (1 << $field_offset)) != 0
                }

                $crate::third_party::paste::paste! {
                    #[allow(dead_code)]
                    pub fn [<set_ $field_name>](&mut self, value: bool) {
                        if value {
                            self.0 |= 1 << $field_offset;
                        } else {
                            self.0 &= !(1 << $field_offset);
                        }
                    }
                }

                $crate::third_party::paste::paste! {
                    #[allow(dead_code)]
                    pub fn [<with_ $field_name>](mut self, value: bool) -> Self {
                        if value {
                            self.0 |= 1 << $field_offset;
                        } else {
                            self.0 &= !(1 << $field_offset);
                        }
                        self
                    }
                }
            )*
        }
    };
}

#[cfg(test)]
mod tests {
    inputs_bitstruct! {
        #[repr(transparent)]
        #[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
        struct InputSnapshot(u8) {
            left: 0,
            right: 1,
            up: 2,
            down: 3,
        }
    }

    #[test]
    fn test_input_snapshot() {
        let mut snapshot = InputSnapshot::default();
        assert_eq!(snapshot.inner(), 0);
        assert!(!snapshot.left());
        assert!(!snapshot.right());
        assert!(!snapshot.up());
        assert!(!snapshot.down());

        snapshot.set_left(true);
        assert_eq!(snapshot.inner(), 1);
        assert!(snapshot.left());
        assert!(!snapshot.right());
        assert!(!snapshot.up());
        assert!(!snapshot.down());

        snapshot.set_right(true);
        assert_eq!(snapshot.inner(), 3);
        assert!(snapshot.left());
        assert!(snapshot.right());
        assert!(!snapshot.up());
        assert!(!snapshot.down());

        snapshot.set_up(true);
        assert_eq!(snapshot.inner(), 7);
        assert!(snapshot.left());
        assert!(snapshot.right());
        assert!(snapshot.up());
        assert!(!snapshot.down());

        snapshot.set_down(true);
        assert_eq!(snapshot.inner(), 15);
        assert!(snapshot.left());
        assert!(snapshot.right());
        assert!(snapshot.up());
        assert!(snapshot.down());

        snapshot.set_right(false);
        snapshot.set_up(false);
        snapshot.set_down(false);
        assert_eq!(snapshot.inner(), 1);
        assert!(snapshot.left());
        assert!(!snapshot.right());
        assert!(!snapshot.up());
        assert!(!snapshot.down());

        let snapshot = InputSnapshot::all();
        assert_eq!(snapshot.inner(), 15);
    }
}
