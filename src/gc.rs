pub use intuicio_data::lifetime::{ValueReadAccess as Read, ValueWriteAccess as Write};
use intuicio_data::{
    lifetime::LifetimeWeakState,
    managed::gc::{DynamicManagedGc, ManagedGc, ManagedGcLifetime},
};
use std::ops::{Deref, DerefMut};

#[derive(Clone)]
pub struct Heartbeat(pub(crate) LifetimeWeakState);

impl Heartbeat {
    pub fn is_alive(&self) -> bool {
        self.0.upgrade().is_some()
    }
}

pub struct Gc<T>(pub ManagedGc<T>);

impl<T: Default> Default for Gc<T> {
    fn default() -> Self {
        Self::new(Default::default())
    }
}

impl<T> Gc<T> {
    pub fn new(value: T) -> Self {
        Self(ManagedGc::new(value))
    }

    pub fn heartbeat(&self) -> Heartbeat {
        Heartbeat(match self.0.lifetime() {
            ManagedGcLifetime::Owned(lifetime) => lifetime.state().downgrade().clone(),
            ManagedGcLifetime::Referenced(lifetime) => lifetime.state().clone(),
        })
    }

    pub fn consume(self) -> Result<T, Self> {
        match self.0.consume() {
            Ok(value) => Ok(value),
            Err(managed) => Err(Self(managed)),
        }
    }

    pub fn reference(&self) -> Self {
        Self(self.0.reference())
    }

    pub fn read(&self) -> Read<'_, T> {
        self.0.read::<false>()
    }

    pub fn write(&mut self) -> Write<'_, T> {
        self.0.write::<false>()
    }

    pub fn set<const LOCKING: bool>(&mut self, value: T) {
        *self.0.write::<LOCKING>() = value;
    }

    pub fn into_dynamic(self) -> DynGc {
        DynGc(self.0.into_dynamic())
    }

    pub fn ptr_eq(this: &Self, other: &Self) -> bool {
        unsafe { this.0.exists() && other.0.exists() && this.0.as_ptr() == other.0.as_ptr() }
    }

    pub fn transfer_ownership(from: &mut Self, to: &mut Self) -> bool {
        from.0.transfer_ownership(&mut to.0)
    }
}

impl<T> Deref for Gc<T> {
    type Target = ManagedGc<T>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for Gc<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> From<ManagedGc<T>> for Gc<T> {
    fn from(value: ManagedGc<T>) -> Self {
        Self(value)
    }
}

impl<T> From<Gc<T>> for ManagedGc<T> {
    fn from(value: Gc<T>) -> Self {
        value.0
    }
}

pub struct DynGc(pub DynamicManagedGc);

impl DynGc {
    pub fn new<T>(value: T) -> Self {
        Self(DynamicManagedGc::new(value))
    }

    pub fn heartbeat(&self) -> Heartbeat {
        Heartbeat(match self.0.lifetime() {
            ManagedGcLifetime::Owned(lifetime) => lifetime.state().downgrade().clone(),
            ManagedGcLifetime::Referenced(lifetime) => lifetime.state().clone(),
        })
    }

    pub fn consume<T>(self) -> Result<T, Self> {
        match self.0.consume::<T>() {
            Ok(value) => Ok(value),
            Err(managed) => Err(Self(managed)),
        }
    }

    pub fn reference(&self) -> Self {
        Self(self.0.reference())
    }

    pub fn read<T>(&self) -> Read<'_, T> {
        self.0.read::<false, T>()
    }

    pub fn write<T>(&mut self) -> Write<'_, T> {
        self.0.write::<false, T>()
    }

    pub fn set<const LOCKING: bool, T>(&mut self, value: T) {
        *self.0.write::<LOCKING, T>() = value;
    }

    pub fn into_typed<T>(self) -> Gc<T> {
        Gc(self.0.into_typed())
    }
}

impl Deref for DynGc {
    type Target = DynamicManagedGc;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for DynGc {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl From<DynamicManagedGc> for DynGc {
    fn from(value: DynamicManagedGc) -> Self {
        Self(value)
    }
}

impl From<DynGc> for DynamicManagedGc {
    fn from(value: DynGc) -> Self {
        value.0
    }
}
