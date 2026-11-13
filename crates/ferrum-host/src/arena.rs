//! 代数槽位池（Generational Arena），见 `docs/01-specs/handle-and-id-design.md` §3.3。

use ferrum_abi::GenerationalHandle;

struct Slot<T> {
    generation: u32,
    value: Option<T>,
}

/// 低 32 位为槽位下标、高 32 位为代数的槽位池。代数从 1 开始，回绕时跳过 0。
pub struct Arena<T> {
    slots: Vec<Slot<T>>,
    free: Vec<u32>,
}

impl<T> Default for Arena<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Arena<T> {
    pub const fn new() -> Self {
        Self {
            slots: Vec::new(),
            free: Vec::new(),
        }
    }

    pub fn insert(&mut self, value: T) -> GenerationalHandle {
        if let Some(index) = self.free.pop() {
            let slot = &mut self.slots[index as usize];
            debug_assert!(slot.value.is_none());
            slot.value = Some(value);
            return GenerationalHandle::new(index, slot.generation).expect("generation is never 0");
        }
        let index = u32::try_from(self.slots.len()).expect("arena slot index overflow");
        self.slots.push(Slot {
            generation: 1,
            value: Some(value),
        });
        GenerationalHandle::new(index, 1).expect("generation is never 0")
    }

    pub fn get(&self, handle: GenerationalHandle) -> Option<&T> {
        let slot = self.slots.get(handle.slot_index() as usize)?;
        if slot.generation != handle.generation() {
            return None;
        }
        slot.value.as_ref()
    }

    pub fn get_mut(&mut self, handle: GenerationalHandle) -> Option<&mut T> {
        let slot = self.slots.get_mut(handle.slot_index() as usize)?;
        if slot.generation != handle.generation() {
            return None;
        }
        slot.value.as_mut()
    }

    /// 移除并递增代数，旧句柄随即失效。
    pub fn remove(&mut self, handle: GenerationalHandle) -> Option<T> {
        let index = handle.slot_index();
        let slot = self.slots.get_mut(index as usize)?;
        if slot.generation != handle.generation() {
            return None;
        }
        let value = slot.value.take()?;
        slot.generation = match slot.generation.wrapping_add(1) {
            0 => 1,
            g => g,
        };
        self.free.push(index);
        Some(value)
    }

    pub fn iter(&self) -> impl Iterator<Item = (GenerationalHandle, &T)> {
        self.slots.iter().enumerate().filter_map(|(i, slot)| {
            let value = slot.value.as_ref()?;
            Some((GenerationalHandle::new(i as u32, slot.generation)?, value))
        })
    }

    pub fn len(&self) -> usize {
        self.slots.len() - self.free.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_handle_is_rejected() {
        let mut arena = Arena::new();
        let a = arena.insert("a");
        assert_eq!(arena.get(a), Some(&"a"));
        assert_eq!(arena.remove(a), Some("a"));
        assert!(arena.get(a).is_none());

        let b = arena.insert("b");
        assert_eq!(b.slot_index(), a.slot_index());
        assert_ne!(b.generation(), a.generation());
        assert!(arena.get(a).is_none());
        assert_eq!(arena.get(b), Some(&"b"));
        assert_eq!(arena.len(), 1);
    }
}
