//! A tiny ECS core: entities are ids, each component type lives in its own sparse storage.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub type Entity = u64;

/// Every value of one component type, keyed by entity.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Storage<T>(BTreeMap<Entity, T>);

impl<T> Default for Storage<T> {
    fn default() -> Self {
        Self(BTreeMap::new())
    }
}

impl<T> Storage<T> {
    pub fn get(&self, e: Entity) -> Option<&T> {
        self.0.get(&e)
    }

    pub fn get_mut(&mut self, e: Entity) -> Option<&mut T> {
        self.0.get_mut(&e)
    }

    pub fn has(&self, e: Entity) -> bool {
        self.0.contains_key(&e)
    }

    pub fn insert(&mut self, e: Entity, value: T) {
        self.0.insert(e, value);
    }

    pub fn remove(&mut self, e: Entity) -> Option<T> {
        self.0.remove(&e)
    }

    pub fn iter(&self) -> impl Iterator<Item = (Entity, &T)> {
        self.0.iter().map(|(e, v)| (*e, v))
    }
}

/// Declares `World`: the entity list, one `Storage` per component, and plain resources.
macro_rules! world {
    (
        components { $($c:ident: $ct:ty),* $(,)? }
        resources { $($r:ident: $rt:ty),* $(,)? }
    ) => {
        #[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
        #[serde(default)]
        pub struct World {
            /// Live entities, back to front.
            pub entities: Vec<$crate::ecs::Entity>,
            pub next_id: $crate::ecs::Entity,
            $(pub $c: $crate::ecs::Storage<$ct>,)*
            $(pub $r: $rt,)*
        }

        impl World {
            /// Drops every component attached to `e`.
            fn strip(&mut self, e: $crate::ecs::Entity) {
                $(self.$c.remove(e);)*
            }

            /// Copies every component of `from` onto `to`.
            fn clone_components(&mut self, from: $crate::ecs::Entity, to: $crate::ecs::Entity) {
                $(if let Some(v) = self.$c.get(from).cloned() {
                    self.$c.insert(to, v);
                })*
            }
        }
    };
}

pub(crate) use world;
