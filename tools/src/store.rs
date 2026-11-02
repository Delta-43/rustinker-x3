//! `x3_core::storage::Store` over a host directory, e.g. a mounted SD card's
//! `/X3/DATA`. Writes the same generation files the device reads.
use std::{
    fs::{self, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};
use x3_core::{
    Error,
    storage::{FileKey, Record, Stage, Store},
};

pub struct DirStore {
    dir: PathBuf,
}

impl DirStore {
    pub fn new(dir: &Path) -> Result<Self, String> {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        Ok(Self {
            dir: dir.to_path_buf(),
        })
    }

    fn path(&self, key: FileKey) -> PathBuf {
        self.dir.join(String::from_utf8_lossy(&key.name()).as_ref())
    }

    /// One more than the highest generation of any DAT or MET file.
    pub fn next_generation(&self) -> Result<u32, String> {
        let mut max = 0u32;
        for e in fs::read_dir(&self.dir).map_err(|e| e.to_string())? {
            let name = e.map_err(|e| e.to_string())?.file_name();
            let name = name.to_string_lossy().to_ascii_uppercase();
            if let Some((base, ext)) = name.split_once('.')
                && matches!(ext, "DAT" | "MET")
                && base.len() == 8
                && let Ok(n) = u32::from_str_radix(base, 16)
            {
                max = max.max(n);
            }
        }
        max.checked_add(1)
            .ok_or_else(|| "generations exhausted".into())
    }
}

impl Store for DirStore {
    fn create(&mut self, key: FileKey) -> Result<(), Error> {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(self.path(key))
            .map(drop)
            .map_err(|_| Error::Storage)
    }
    fn append(&mut self, key: FileKey, bytes: &[u8]) -> Result<(), Error> {
        let mut f = OpenOptions::new()
            .append(true)
            .open(self.path(key))
            .map_err(|_| Error::Storage)?;
        f.write_all(bytes).map_err(|_| Error::Storage)
    }
    fn sync(&mut self, key: FileKey) -> Result<(), Error> {
        fs::File::open(self.path(key))
            .and_then(|f| f.sync_all())
            .map_err(|_| Error::Storage)
    }
    fn len(&mut self, key: FileKey) -> Result<usize, Error> {
        fs::metadata(self.path(key))
            .map(|m| m.len() as usize)
            .map_err(|_| Error::Storage)
    }
    fn read(&mut self, key: FileKey, offset: usize, out: &mut [u8]) -> Result<usize, Error> {
        let mut f = fs::File::open(self.path(key)).map_err(|_| Error::Storage)?;
        f.seek(SeekFrom::Start(offset as u64))
            .map_err(|_| Error::Storage)?;
        f.read(out).map_err(|_| Error::Storage)
    }
}

/// Stores one validated `ar1` payload as item `id` and returns its record.
pub fn put_article(dir: &Path, id: &str, ar1: &[u8]) -> Result<Record, String> {
    use sha2::{Digest, Sha256};
    x3_proto::payload::Article::decode(ar1).map_err(|e| format!("not a valid ar1: {e:?}"))?;
    let mut store = DirStore::new(dir)?;
    let generation = store.next_generation()?;
    let sha: [u8; 32] = Sha256::digest(ar1).into();
    let record = Record::new(id, generation, ar1.len() as u32, sha, false)
        .map_err(|e| format!("record: {e:?}"))?;
    let mut stage = Stage::begin(&mut store, record).map_err(|e| format!("stage: {e:?}"))?;
    for chunk in ar1.chunks(4096) {
        stage
            .write(&mut store, chunk)
            .map_err(|e| format!("write: {e:?}"))?;
    }
    stage
        .commit(&mut store)
        .map_err(|e| format!("commit: {e:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use x3_core::{
        library::Shelf,
        storage::{RECORD_LEN, recover},
    };

    #[test]
    fn put_article_round_trips_through_storage_and_shelf() {
        let dir = std::env::temp_dir().join(format!("x3-put-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let ar1 = crate::article("Tytuł", "", "# Nagłówek\n\nTreść.\n", false, 0).unwrap();
        let first = put_article(&dir, "art-1", &ar1).unwrap();
        let second = put_article(&dir, "art-1", &ar1).unwrap();
        assert_eq!((first.generation, second.generation), (1, 2));
        let mut store = DirStore::new(&dir).unwrap();
        let latest = recover(&mut store, "art-1", [1, 2]).unwrap().unwrap();
        assert_eq!(latest.generation, 2);
        // The shelf sees one article, its newest record.
        let mut shelf = Shelf::default();
        for g in [1u32, 2] {
            let key = FileKey {
                generation: g,
                kind: x3_core::storage::FileKind::Metadata,
            };
            let mut b = [0u8; RECORD_LEN];
            assert_eq!(store.read(key, 0, &mut b).unwrap(), RECORD_LEN);
            shelf.add(Record::decode(&b).unwrap());
        }
        assert_eq!(shelf.len(), 1);
        assert_eq!(shelf.articles().next().unwrap().generation, 2);
        // A position save is a new generation and verifies.
        let moved = x3_core::storage::position(&mut store, &latest, 3, 3).unwrap();
        assert_eq!(moved.position, 3);
        assert!(put_article(&dir, "bad id!", &ar1).is_err());
        assert!(put_article(&dir, "x", b"not ar1").is_err());
        fs::remove_dir_all(&dir).unwrap();
    }
}
