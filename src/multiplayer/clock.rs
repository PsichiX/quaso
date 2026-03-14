use std::error::Error;
use tehuti::{
    channel::{ChannelId, ChannelMode, Dispatch},
    codec::postcard::PostcardCodec,
    event::Duplex,
    peer::{PeerBuilder, PeerDestructurer, TypedPeer},
};
use tehuti_client_server::authority::AuthorityUserData;
use tehuti_timeline::clock::{Clock, ClockEvent};

pub struct ClockExtension<const AUTHORITY_CLOCK_CHANNEL: u64> {
    pub clock: Clock,
    pub events: Duplex<Dispatch<ClockEvent>>,
}

impl<const AUTHORITY_CLOCK_CHANNEL: u64> TypedPeer for ClockExtension<AUTHORITY_CLOCK_CHANNEL> {
    fn builder(builder: PeerBuilder) -> Result<PeerBuilder, Box<dyn Error>> {
        Ok(
            builder.bind_read_write::<PostcardCodec<ClockEvent>, ClockEvent>(
                ChannelId::new(AUTHORITY_CLOCK_CHANNEL),
                ChannelMode::Unreliable,
                None,
            ),
        )
    }

    fn into_typed(mut peer: PeerDestructurer) -> Result<Self, Box<dyn Error>> {
        let is_server = peer.user_data().access::<AuthorityUserData>()?.is_server;
        let events = peer.read_write::<ClockEvent>(ChannelId::new(AUTHORITY_CLOCK_CHANNEL))?;

        Ok(Self {
            clock: if is_server {
                Clock::Authority(Default::default())
            } else {
                Clock::Simulation(Default::default())
            },
            events,
        })
    }
}
