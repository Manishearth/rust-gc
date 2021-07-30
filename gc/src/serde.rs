use crate::{Gc, Trace};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

impl<'de, T: Deserialize<'de> + Trace> Deserialize<'de> for Gc<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        T::deserialize(deserializer).map(Gc::new)
    }
}

impl<T: Serialize + Trace> Serialize for Gc<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        T::serialize(&self, serializer)
    }
}
