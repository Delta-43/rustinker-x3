const RECORD: usize = 66;
use x3_core::Error;
extern crate std;
use std::{
    collections::BTreeMap,
    string::{String, ToString},
    vec::Vec,
};
use x3_core::cache::*;
#[derive(Default)]
struct Memory {
    files: BTreeMap<String, Vec<u8>>,
    fail_at: Option<usize>,
    calls: usize,
}
impl Memory {
    fn step(&mut self) -> Result<(), Error> {
        self.calls += 1;
        if self.fail_at == Some(self.calls) {
            Err(Error::Storage)
        } else {
            Ok(())
        }
    }
}
impl Storage for Memory {
    fn write(&mut self, n: &str, b: &[u8]) -> Result<(), Error> {
        if let Err(e) = self.step() {
            self.files.insert(n.to_string(), b[..b.len() / 2].to_vec());
            return Err(e);
        }
        self.files.insert(n.to_string(), b.to_vec());
        Ok(())
    }
    fn flush(&mut self, _: &str) -> Result<(), Error> {
        self.step()
    }
    fn rename(&mut self, f: &str, t: &str) -> Result<(), Error> {
        self.step()?;
        let data = self.files.remove(f).ok_or(Error::Storage)?;
        self.files.insert(t.to_string(), data);
        Ok(())
    }
    fn read(&self, n: &str, out: &mut [u8]) -> Result<usize, Error> {
        let b = self.files.get(n).ok_or(Error::Storage)?;
        if b.len() > out.len() {
            return Err(Error::BufferTooSmall);
        }
        out[..b.len()].copy_from_slice(b);
        Ok(b.len())
    }
    fn remove(&mut self, n: &str) -> Result<(), Error> {
        self.files.remove(n);
        Ok(())
    }
}
#[test]
fn paths_are_bounded_and_traversal_rejected() {
    for p in [
        "", "..", ".", "../foo", "a/b", "a\\b", "/root", "a%2fb", "a\0b", "ą",
    ] {
        assert!(!valid_id(p));
    }
    assert!(valid_id("Article_01-a"));
    assert!(!valid_id(&"x".repeat(49)));
    let mut out = [0; 48];
    let n = sanitise_filename("Zażółć.txt", &mut out).unwrap();
    assert_eq!(&out[..n], b"Za_____txt");
    for p in ["", "..", "a/b", "a\\b"] {
        assert!(sanitise_filename(p, &mut out).is_err());
    }
    assert!(sanitise_filename("a", &mut []).is_err());
    assert!(sanitise_filename(&"a".repeat(257), &mut out).is_err());
}
#[test]
fn eviction_prefers_oldest_read_then_oldest_unread() {
    let mut i = Index::<3>::new(20).unwrap();
    i.insert(Entry::new("a", 1, 8).unwrap(), |_| {}).unwrap();
    let mut b = Entry::new("b", 2, 8).unwrap();
    b.read = true;
    i.insert(b, |_| {}).unwrap();
    let mut ev = Vec::new();
    i.insert(Entry::new("c", 3, 8).unwrap(), |e| {
        ev.push(e.id().to_string())
    })
    .unwrap();
    assert_eq!(ev, ["b"]);
    i.insert(Entry::new("d", 4, 8).unwrap(), |e| {
        ev.push(e.id().to_string())
    })
    .unwrap();
    assert_eq!(ev, ["b", "a"]);
    let before = i.used();
    assert!(
        i.insert(Entry::new("huge", 0, 21).unwrap(), |_| {})
            .is_err()
    );
    assert_eq!(before, i.used());
    i.insert(Entry::new("d", 5, 4).unwrap(), |_| {}).unwrap();
    assert_eq!(i.entries().count(), 2);
    assert!(i.remove("../d").is_err());
    assert_eq!(i.remove("d").unwrap().size, 4);
    assert!(i.remove("d").is_err());
}
#[test]
fn index_reload_corruption_and_all_truncations() {
    let mut i = Index::<2>::new(50).unwrap();
    let mut a = Entry::new("a", 1, 8).unwrap();
    a.set_position("ąb", 2).unwrap();
    assert!(a.set_position("ąb", 1).is_err());
    i.insert(a, |_| {}).unwrap();
    let mut out = [0; 200];
    let n = i.encode(&mut out).unwrap();
    let loaded = Index::<2>::decode(&out[..n], 50).unwrap();
    assert_eq!(*loaded.entries().next().unwrap(), a);
    for size in 0..n {
        assert!(Index::<2>::decode(&out[..size], 50).is_err());
    }
    for at in 0..n {
        out[at] ^= 1;
        assert!(Index::<2>::decode(&out[..n], 50).is_err());
        out[at] ^= 1;
    }
    assert!(Index::<2>::decode(&out[..n], 1).is_err());
    assert!(i.encode(&mut []).is_err());
    assert!(Index::<0>::new(1).is_err());
    assert!(Index::<501>::new(1).is_err());
    assert!(Index::<2>::new(0).is_err());
    assert_eq!(crc32(b"123456789"), 0xcbf43926);
}
#[test]
fn interrupted_write_preserves_committed_file() {
    for fail in 1..=3 {
        let mut s = Memory::default();
        atomic_write(&mut s, "item", b"old").unwrap();
        s.calls = 0;
        s.fail_at = Some(fail);
        assert_eq!(
            atomic_write(&mut s, "item", b"replacement"),
            Err(Error::Storage)
        );
        let mut out = [0; 32];
        let n = s.read("item", &mut out).unwrap();
        assert_eq!(&out[..n], b"old");
        s.fail_at = None;
        atomic_write(&mut s, "item", b"new").unwrap();
        let n = s.read("item", &mut out).unwrap();
        assert_eq!(&out[..n], b"new");
    }
    let mut s = Memory::default();
    assert!(atomic_write(&mut s, "../item", b"x").is_err());
    assert!(atomic_write(&mut s, "item", &std::vec![0;264_477]).is_err());
}
#[test]
fn five_hundred_synthetic_entries_and_repeated_eviction() {
    let mut i = Index::<500>::new(5000).unwrap();
    for n in 0..1000 {
        let id = std::format!("item{n}");
        i.insert(Entry::new(&id, n, 10).unwrap(), |_| {}).unwrap();
    }
    assert_eq!(i.entries().count(), 500);
    assert_eq!(i.used(), 5000);
    assert_eq!(i.entries().map(|e| e.created).min(), Some(500));
    let mut bytes = std::vec![0;40_000];
    let n = i.encode(&mut bytes).unwrap();
    assert_eq!(
        Index::<500>::decode(&bytes[..n], 5000)
            .unwrap()
            .entries()
            .count(),
        500
    );
}
#[test]
fn hostile_index_with_recomputed_checksums_is_rejected() {
    let mut index = Index::<2>::new(100).unwrap();
    index
        .insert(Entry::new("a", 1, 10).unwrap(), |_| {})
        .unwrap();
    index
        .insert(Entry::new("b", 2, 10).unwrap(), |_| {})
        .unwrap();
    let mut original = [0; 200];
    let n = index.encode(&mut original).unwrap();
    // These mutations pass CRC but violate structure or semantic bounds.
    for (offset, value) in [
        (6 + 48, 0),
        (6 + 48, 49),
        (6, b'/'),
        (7, 1),
        (6 + 65, 2),
        (6 + 63, 255),
        (6 + 57, 255),
        (6 + 58, 255),
        (6 + 59, 255),
        (6 + RECORD, b'a'),
    ] {
        let mut bytes = original;
        bytes[offset] = value;
        let crc = crc32(&bytes[..n - 4]);
        bytes[n - 4..n].copy_from_slice(&crc.to_le_bytes());
        assert!(
            Index::<2>::decode(&bytes[..n], 100).is_err(),
            "offset {offset}"
        );
    }
    let mut e = Entry::new("position", 0, 1).unwrap();
    e.position = 262_145;
    assert!(index.insert(e, |_| {}).is_err());
    assert!(Entry::new("x", 0, 264_477).is_err());
    assert!(Entry::new("../x", 0, 1).is_err());
}
#[test]
fn bounded_random_cache_sequence_preserves_capacity_and_unique_ids() {
    let mut index = Index::<8>::new(80).unwrap();
    let mut seed = 1u32;
    for n in 0..2000 {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let id = std::format!("id{}", seed % 16);
        let mut e = Entry::new(&id, n, seed % 20 + 1).unwrap();
        e.read = seed & 16 != 0;
        index.insert(e, |_| {}).unwrap();
        assert!(index.used() <= 80);
        assert!(index.entries().count() <= 8);
        for a in index.entries() {
            assert_eq!(index.entries().filter(|b| b.id() == a.id()).count(), 1);
        }
        let mut bytes = [0; 600];
        let len = index.encode(&mut bytes).unwrap();
        index = Index::<8>::decode(&bytes[..len], 80).unwrap();
    }
}

#[test]
fn damaged_index_recovers_to_an_empty_usable_library() {
    let (mut recovered, repaired) = Index::<2>::recover(b"partial index", 100).unwrap();
    assert!(repaired);
    assert_eq!(recovered.entries().count(), 0);
    recovered
        .insert(Entry::new("new", 1, 10).unwrap(), |_| {})
        .unwrap();
    let mut bytes = [0; 100];
    let n = recovered.encode(&mut bytes).unwrap();
    let (loaded, repaired) = Index::<2>::recover(&bytes[..n], 100).unwrap();
    assert!(!repaired);
    assert_eq!(loaded.entries().count(), 1);
    assert!(Index::<2>::recover(b"", 0).is_err());
}
