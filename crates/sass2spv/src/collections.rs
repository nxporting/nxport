// Hash collections that iterate like .NET's Dictionary and HashSet: entries in slot order, a removed entry's slot
// reused by the next insertion (most recently freed first). The translator's output depends on iteration order in
// several places, so it must match the original's exactly.

use std::collections::HashMap;
use std::hash::Hash;

#[derive(Clone, Debug)]
pub struct NetMap<K, V> {
    slots: Vec<Option<(K, V)>>,
    free: Vec<usize>,
    index: HashMap<K, usize>,
}

impl<K: Clone + Eq + Hash, V> Default for NetMap<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Clone + Eq + Hash, V> NetMap<K, V> {
    pub fn new() -> Self {
        NetMap { slots: Vec::new(), free: Vec::new(), index: HashMap::new() }
    }

    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    pub fn contains_key(&self, k: &K) -> bool {
        self.index.contains_key(k)
    }

    pub fn get(&self, k: &K) -> Option<&V> {
        self.index.get(k).map(|&i| &self.slots[i].as_ref().unwrap().1)
    }

    pub fn get_mut(&mut self, k: &K) -> Option<&mut V> {
        let i = *self.index.get(k)?;
        Some(&mut self.slots[i].as_mut().unwrap().1)
    }

    /// Adds `k` if absent (Dictionary.TryAdd); returns whether it was added.
    pub fn try_add(&mut self, k: K, v: V) -> bool {
        if self.index.contains_key(&k) {
            return false;
        }
        let i = match self.free.pop() {
            Some(i) => i,
            None => {
                self.slots.push(None);
                self.slots.len() - 1
            }
        };
        self.index.insert(k.clone(), i);
        self.slots[i] = Some((k, v));
        true
    }

    /// Dictionary indexer assignment: replaces the value in place, or adds.
    pub fn set(&mut self, k: K, v: V) {
        match self.index.get(&k) {
            Some(&i) => self.slots[i].as_mut().unwrap().1 = v,
            None => {
                self.try_add(k, v);
            }
        }
    }

    pub fn remove(&mut self, k: &K) -> Option<V> {
        let i = self.index.remove(k)?;
        self.free.push(i);
        self.slots[i].take().map(|e| e.1)
    }

    pub fn clear(&mut self) {
        self.slots.clear();
        self.free.clear();
        self.index.clear();
    }

    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.slots.iter().flatten().map(|(k, v)| (k, v))
    }

    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.iter().map(|e| e.0)
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.iter().map(|e| e.1)
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.slots.iter_mut().flatten().map(|(_, v)| v)
    }
}

#[derive(Clone, Debug)]
pub struct NetSet<T> {
    map: NetMap<T, ()>,
}

impl<T: Clone + Eq + Hash> Default for NetSet<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone + Eq + Hash> NetSet<T> {
    pub fn new() -> Self {
        NetSet { map: NetMap::new() }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn contains(&self, v: &T) -> bool {
        self.map.contains_key(v)
    }

    /// HashSet.Add: returns whether `v` was added.
    pub fn insert(&mut self, v: T) -> bool {
        self.map.try_add(v, ())
    }

    pub fn remove(&mut self, v: &T) -> bool {
        self.map.remove(v).is_some()
    }

    pub fn clear(&mut self) {
        self.map.clear();
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.map.keys()
    }

    /// The elements in iteration order, for loops that change the set or what it refers to.
    pub fn to_vec(&self) -> Vec<T> {
        self.iter().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freed_slots_are_reused_last_freed_first() {
        let mut s = NetSet::new();
        for v in [1, 2, 3, 4] {
            s.insert(v);
        }
        s.remove(&2);
        s.remove(&4);
        s.insert(5); // takes 4's slot
        s.insert(6); // takes 2's slot
        s.insert(7);
        assert_eq!(s.to_vec(), vec![1, 6, 3, 5, 7]);
    }
}
