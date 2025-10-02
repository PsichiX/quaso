use crate::game::{enemy::EnemyState, item::Item};
use quaso::{
    third_party::{
        rstar::{Envelope, Point, PointDistance, RTree, RTreeObject, AABB},
        typid::ID,
        vek::Vec2,
    },
    value::{Ptr, Val},
};
use std::cell::RefCell;

thread_local! {
    static INSTANCE: RefCell<Val<Space>> = Default::default();
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum SpaceObjectId {
    #[default]
    None,
    Player,
    Enemy(ID<EnemyState>),
    Item(ID<Item>),
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct SpaceObject {
    pub id: SpaceObjectId,
    pub position: Vec2<f32>,
    pub collider_radius: f32,
}

impl SpaceObject {
    pub fn does_collide_broad(&self, other: &Self) -> bool {
        self.envelope().intersects(&other.envelope())
    }

    pub fn does_collide_narrow(&self, other: &Self) -> bool {
        let a = self.collider_radius * self.collider_radius;
        let b = other.collider_radius * other.collider_radius;
        self.position.distance_squared(other.position) <= a + b
    }
}

impl RTreeObject for SpaceObject {
    type Envelope = AABB<[f32; 2]>;

    fn envelope(&self) -> Self::Envelope {
        AABB::from_corners(
            [
                self.position.x - self.collider_radius,
                self.position.y - self.collider_radius,
            ],
            [
                self.position.x + self.collider_radius,
                self.position.y + self.collider_radius,
            ],
        )
    }
}

impl PointDistance for SpaceObject {
    fn distance_2(
        &self,
        point: &<Self::Envelope as Envelope>::Point,
    ) -> <<Self::Envelope as Envelope>::Point as Point>::Scalar {
        self.envelope().distance_2(point)
    }
}

#[derive(Debug, Default)]
pub struct Space {
    tree: RTree<SpaceObject>,
}

impl Space {
    pub fn access() -> Ptr<Self> {
        INSTANCE.with(|instance| instance.borrow().pointer())
    }

    pub fn maintain(&mut self, objects: Vec<SpaceObject>) {
        self.tree = RTree::bulk_load(objects);
    }

    pub fn nearest(&self, position: Vec2<f32>) -> impl Iterator<Item = &SpaceObject> {
        self.tree.nearest_neighbor_iter(&[position.x, position.y])
    }

    pub fn nearest_in_range(
        &self,
        position: Vec2<f32>,
        range: f32,
    ) -> impl Iterator<Item = &SpaceObject> {
        let range_sqr = range * range;
        self.nearest(position)
            .take_while(move |object| object.position.distance_squared(position) <= range_sqr)
    }

    pub fn collisions_by_id(
        &self,
        id: SpaceObjectId,
        narrow: bool,
    ) -> Option<impl Iterator<Item = &SpaceObject>> {
        self.find_by_id(id)
            .map(|object| self.collisions(object, narrow))
    }

    pub fn collisions<'a>(
        &'a self,
        object: &'a SpaceObject,
        narrow: bool,
    ) -> impl Iterator<Item = &'a SpaceObject> {
        self.tree
            .locate_in_envelope_intersecting(&object.envelope())
            .filter(move |item| !narrow || object.does_collide_narrow(item))
    }

    pub fn find_by_id(&self, id: SpaceObjectId) -> Option<&SpaceObject> {
        self.tree.iter().find(|object| object.id == id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &SpaceObject> {
        self.tree.iter()
    }
}
