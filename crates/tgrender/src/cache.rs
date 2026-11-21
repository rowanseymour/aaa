//! Loading things once, however often they're asked for.

use std::borrow::Borrow;
use std::collections::HashMap;
use std::hash::Hash;

/// The value cached for `key`, loading and caching it on first use.
pub(crate) fn cached<K, Q, T>(
    cache: &mut HashMap<K, Option<T>>,
    key: &Q,
    load: impl FnOnce() -> Option<T>,
) -> Option<T>
where
    K: Borrow<Q> + Hash + Eq,
    Q: ToOwned<Owned = K> + Hash + Eq + ?Sized,
    T: Clone,
{
    if let Some(v) = cache.get(key) {
        return v.clone();
    }
    let v = load();
    cache.insert(key.to_owned(), v.clone());
    v
}
