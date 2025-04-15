use crate::game::{enemy::EnemyState, item::Item};
use quaso::third_party::{typid::ID, vek::Vec2};
use std::{borrow::Cow, cell::RefCell};

thread_local! {
    static PENDING: RefCell<Vec<Event>> = Default::default();
    static RECEIVED: RefCell<Vec<Event>> = Default::default();
    static DELAYED: RefCell<Vec<(f32, Event)>> = Default::default();
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Instigator {
    Player,
    Enemy,
}

pub enum Event {
    KillPlayer,
    KillEnemy {
        id: ID<EnemyState>,
    },
    KillItem {
        id: ID<Item>,
    },
    Attack {
        position: Vec2<f32>,
        range: f32,
        value: usize,
        instigator: Instigator,
    },
    WinGame,
    PlaySound(Cow<'static, str>),
}

#[derive(Default)]
pub struct Events;

impl Events {
    pub fn write(event: Event) {
        PENDING.with(|pending| pending.borrow_mut().push(event));
    }

    pub fn write_delayed(seconds: f32, event: Event) {
        DELAYED.with(|delayed| delayed.borrow_mut().push((seconds, event)));
    }

    pub fn read<R>(mut f: impl FnMut(&[Event]) -> R) -> R {
        RECEIVED.with(|received| f(&received.borrow()))
    }

    pub fn maintain(delta_time: f32) {
        PENDING.with(|pending| {
            RECEIVED.with(|received| {
                DELAYED.with(|delayed| {
                    let pending: &mut Vec<_> = &mut pending.borrow_mut();
                    let received: &mut Vec<_> = &mut received.borrow_mut();
                    let delayed: &mut Vec<_> = &mut delayed.borrow_mut();
                    std::mem::swap(pending, received);
                    pending.clear();
                    *delayed = std::mem::take(delayed)
                        .into_iter()
                        .filter_map(|(mut seconds, event)| {
                            seconds -= delta_time;
                            if seconds <= 0.0 {
                                received.push(event);
                                None
                            } else {
                                Some((seconds, event))
                            }
                        })
                        .collect();
                });
            });
        });
    }
}
