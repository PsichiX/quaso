use intuicio_data::{
    lifetime::{Lifetime, LifetimeLazy, LifetimeWeakState},
    managed::{DynamicManaged, DynamicManagedLazy, Managed, ManagedLazy},
};

pub use intuicio_data::lifetime::{ValueReadAccess as Read, ValueWriteAccess as Write};

#[derive(Clone)]
pub struct Heartbeat(pub(crate) LifetimeWeakState);

pub struct Val<T>(Box<Managed<T>>);

impl<T> Default for Val<T>
where
    T: Default,
{
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T> Val<T> {
    pub fn new(value: T) -> Self {
        Self(Box::new(Managed::new(value)))
    }

    pub fn new_raw(value: Managed<T>) -> Self {
        Self(Box::new(value))
    }

    pub fn into_inner(self) -> Managed<T> {
        *self.0
    }

    pub fn heartbeat(&self) -> Heartbeat {
        Heartbeat(self.0.lifetime().state().downgrade().clone())
    }

    pub fn lifetime(&self) -> &Lifetime {
        self.0.lifetime()
    }

    pub fn pointer(&self) -> Ptr<T> {
        unsafe { Ptr(self.0.lazy_immutable()) }
    }

    pub fn read(&self) -> Read<'_, T> {
        self.0.read().expect("Could not read value")
    }

    pub fn read_checked(&self) -> Option<Read<'_, T>> {
        self.0.read()
    }

    pub fn write(&mut self) -> Write<'_, T> {
        self.0.write().expect("Could not write value")
    }

    pub fn write_checked(&mut self) -> Option<Write<'_, T>> {
        self.0.write()
    }

    pub fn set(&mut self, value: T)
    where
        T: Sized,
    {
        *self.write() = value;
    }

    pub fn into_dynamic(self) -> DynVal {
        DynVal::new_raw(
            (*self.0)
                .into_dynamic()
                .expect("Could not convert to dynamic managed value"),
        )
    }
}

pub struct Ptr<T: ?Sized>(ManagedLazy<T>);

impl<T: ?Sized> Clone for Ptr<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: ?Sized> Ptr<T> {
    pub fn make(value: &mut T) -> (Self, Lifetime) {
        let (lazy, lifetime) = ManagedLazy::make(value);
        (Self(lazy), lifetime)
    }

    pub fn new_raw(value: ManagedLazy<T>) -> Self {
        Self(value)
    }

    pub fn into_inner(self) -> ManagedLazy<T> {
        self.0
    }

    pub fn heartbeat(&self) -> Heartbeat {
        Heartbeat(self.0.lifetime().state().clone())
    }

    pub fn lifetime(&self) -> &LifetimeLazy {
        self.0.lifetime()
    }

    pub fn read(&self) -> Read<'_, T> {
        self.0.read().expect("Could not read value")
    }

    pub fn read_checked(&self) -> Option<Read<'_, T>> {
        self.0.read()
    }

    pub fn write(&self) -> Write<'_, T> {
        self.0.write().expect("Could not write value")
    }

    pub fn write_checked(&self) -> Option<Write<'_, T>> {
        self.0.write()
    }

    pub fn set(&self, value: T)
    where
        T: Sized,
    {
        *self.write() = value;
    }

    pub fn into_dynamic(self) -> DynPtr {
        DynPtr::new_raw(self.0.into_dynamic())
    }
}

pub struct DynVal(DynamicManaged);

impl DynVal {
    pub fn new<T>(value: T) -> Self {
        Self(
            DynamicManaged::new(value)
                .ok()
                .expect("Could not create dynamic managed value"),
        )
    }

    pub fn new_raw(value: DynamicManaged) -> Self {
        Self(value)
    }

    pub fn into_inner(self) -> DynamicManaged {
        self.0
    }

    pub fn heartbeat(&self) -> Heartbeat {
        Heartbeat(self.0.lifetime().state().downgrade().clone())
    }

    pub fn lifetime(&self) -> &Lifetime {
        self.0.lifetime()
    }

    pub fn pointer(&self) -> DynPtr {
        DynPtr(self.0.lazy())
    }

    pub fn read<T>(&self) -> Read<'_, T> {
        self.0.read::<T>().expect("Could not read value")
    }

    pub fn read_checked<T>(&self) -> Option<Read<'_, T>> {
        self.0.read::<T>()
    }

    pub fn write<T>(&mut self) -> Write<'_, T> {
        self.0.write::<T>().expect("Could not write value")
    }

    pub fn write_checked<T>(&mut self) -> Option<Write<'_, T>> {
        self.0.write::<T>()
    }

    pub fn set<T>(&mut self, value: T)
    where
        T: Sized,
    {
        *self.write() = value;
    }

    pub fn into_typed<T>(self) -> Val<T> {
        Val::new_raw(
            self.0
                .into_typed()
                .ok()
                .expect("Could not convert to typed managed value"),
        )
    }
}

pub struct DynPtr(DynamicManagedLazy);

impl Clone for DynPtr {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl DynPtr {
    pub fn make<T: ?Sized>(value: &mut T) -> (Self, Lifetime) {
        let (lazy, lifetime) = DynamicManagedLazy::make(value);
        (Self(lazy), lifetime)
    }

    pub fn new_raw(value: DynamicManagedLazy) -> Self {
        Self(value)
    }

    pub fn into_inner(self) -> DynamicManagedLazy {
        self.0
    }

    pub fn heartbeat(&self) -> Heartbeat {
        Heartbeat(self.0.lifetime().state().clone())
    }

    pub fn lifetime(&self) -> &LifetimeLazy {
        self.0.lifetime()
    }

    pub fn read<T>(&self) -> Read<'_, T> {
        self.0.read::<T>().expect("Could not read value")
    }

    pub fn read_checked<T>(&self) -> Option<Read<'_, T>> {
        self.0.read::<T>()
    }

    pub fn write<T>(&self) -> Write<'_, T> {
        self.0.write::<T>().expect("Could not write value")
    }

    pub fn write_checked<T>(&self) -> Option<Write<'_, T>> {
        self.0.write::<T>()
    }

    pub fn set<T>(&self, value: T)
    where
        T: Sized,
    {
        *self.write() = value;
    }

    pub fn into_typed<T>(self) -> Ptr<T> {
        Ptr::new_raw(
            self.0
                .into_typed()
                .ok()
                .expect("Could not convert to typed managed value"),
        )
    }
}
