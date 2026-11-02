#![cfg(feature = "fat")]
mod support;
use embedded_sdmmc::{VolumeIdx, VolumeManager};
use sha2::{Digest, Sha256};
use support::*;
use x3_core::{Error, fat::FatStore, storage::*};
fn article(text: &str) -> Vec<u8> {
    let mut b = vec![0; 300000];
    let n =
        x3_proto::payload::encode_article("Polski", "https://example.org/", text, false, 0, &mut b)
            .unwrap();
    b.truncate(n);
    b
}
fn record(g: u32, b: &[u8]) -> Record {
    Record::new("a-one", g, b.len() as u32, Sha256::digest(b).into(), false).unwrap()
}
fn install(s: &mut impl Store, g: u32, b: &[u8]) -> Result<Record, Error> {
    let mut stage = Stage::begin(s, record(g, b))?;
    for chunk in b.chunks(127) {
        stage.write(s, chunk)?;
    }
    stage.commit(s)
}
fn mounted<T>(disk: Disk, f: impl FnOnce(&mut FatStore<'_, Disk, Clock>) -> T) -> T {
    let m = VolumeManager::new(disk, Clock);
    let v = m.open_raw_volume(VolumeIdx(0)).unwrap();
    let d = m.open_root_dir(v).unwrap();
    f(&mut FatStore {
        manager: &m,
        directory: d,
    })
}
#[test]
fn replacement_position_and_rebuild() {
    let disk = Disk::new(blank());
    let old = article("Zażółć gęślą jaźń\n");
    let new = article("replacement");
    mounted(disk.clone(), |s| {
        assert_eq!(s.next_generation().unwrap(), 1);
        let r = install(s, 1, &old).unwrap();
        let p = position(s, &r, 2, 2).unwrap();
        assert_eq!(p.position, 2);
        assert!(position(s, &r, 3, 3).is_err());
        let r = install(s, 4, &new).unwrap();
        assert_eq!(r.position, 0);
        let mut ids = [0; 2];
        assert_eq!(s.generations(0, &mut ids).unwrap(), 2);
        assert_eq!(ids, [1, 2]);
        assert_eq!(s.generations(2, &mut ids).unwrap(), 1);
        assert_eq!(ids[0], 4);
        assert_eq!(s.next_generation().unwrap(), 5);
    });
    mounted(disk, |s| {
        let r = recover(s, "a-one", 1..=5).unwrap().unwrap();
        assert_eq!(r.generation, 4);
        assert_eq!(r.sha256, <[u8; 32]>::from(Sha256::digest(&new)));
        let deleted = delete(s, &r, 5).unwrap();
        assert!(deleted.deleted);
        assert!(recover(s, "a-one", 1..=5).unwrap().unwrap().deleted);
    });
}
#[test]
fn cut_at_each_fat_write_preserves_old_or_new() {
    let original = article("old article");
    let replacement = article(&"New Polish: ąęłńóśźż\n".repeat(40));
    let disk = Disk::new(blank());
    mounted(disk.clone(), |s| {
        install(s, 1, &original).unwrap();
    });
    let base = disk.0.borrow().bytes.clone();
    let complete = Disk::new(base.clone());
    mounted(complete.clone(), |s| {
        install(s, 2, &replacement).unwrap();
    });
    let writes = complete.0.borrow().writes;
    assert!(writes > 10);
    for cut in 0..writes {
        for prefix in [0, 1, 127, 255, 511, 512] {
            let d = Disk::new(base.clone());
            {
                let mut state = d.0.borrow_mut();
                state.cut = Some(cut);
                state.prefix = prefix;
            }
            mounted(d.clone(), |s| {
                assert!(install(s, 2, &replacement).is_err());
            });
            let recovered = Disk::new(d.0.borrow().bytes.clone());
            mounted(recovered, |s| {
                let r = recover(s, "a-one", 1..=2)
                    .unwrap()
                    .expect("old or new remains");
                assert!(r.generation == 1 || r.generation == 2);
                verify(s, &r).unwrap();
            });
        }
    }
    println!(
        "tested {writes} FAT block writes at 6 cut prefixes ({} power cuts)",
        writes * 6
    );
}
#[test]
fn malformed_staging_and_records() {
    let disk = Disk::new(blank());
    mounted(disk, |s| {
        let bytes = article("valid");
        let r = record(1, &bytes);
        let encoded = r.encode().unwrap();
        assert_eq!(Record::decode(&encoded).unwrap(), r);
        for n in 0..RECORD_LEN {
            assert!(Record::decode(&encoded[..n]).is_err());
        }
        for n in 0..RECORD_LEN {
            let mut bad = encoded;
            bad[n] ^= 1;
            assert!(Record::decode(&bad).is_err());
        }
        let mut stage = Stage::begin(s, r.clone()).unwrap();
        assert!(stage.write(s, &vec![0; bytes.len() + 1]).is_err());
        assert!(stage.write(s, b"x").is_err());
        assert!(stage.commit(s).is_err());
        assert!(Stage::begin(s, r).is_err());
        let mut stage = Stage::begin(s, record(2, &bytes)).unwrap();
        stage.write(s, &bytes[..10]).unwrap();
        assert!(stage.commit(s).is_err());
        assert!(recover(s, "a-one", 1..=2).unwrap().is_none());
        let mut bad = bytes.clone();
        bad[28] = 255;
        assert!(install(s, 3, &bad).is_err());
        let mut bad = bytes.clone();
        let n = bad.len();
        bad[n - 1] = 0;
        let crc = x3_proto::payload::crc32(&bad[28..]);
        bad[24..28].copy_from_slice(&crc.to_le_bytes());
        assert!(install(s, 4, &bad).is_err());
        assert!(Record::new("../bad", 1, 10, [0; 32], false).is_err());
        assert!(Record::new("a", 0, 10, [0; 32], false).is_err());
        assert!(Record::new("a", 1, 300000, [0; 32], false).is_err());
    });
}
#[test]
fn token_cache_protects_screen() {
    let mut records = Vec::new();
    for n in 1..=21 {
        records.push(Record::new(&format!("t-{n}"), n, 52288, [0; 32], true).unwrap());
    }
    assert_eq!(
        token_victim(&records, "t-1", 20).unwrap().unwrap().item(),
        "t-2"
    );
    assert!(token_victim(&records[..20], "t-1", 20).unwrap().is_none());
    assert!(token_victim(&records, "t-1", 0).is_err());
}

#[test]
fn cut_at_each_position_and_delete_write() {
    let disk = Disk::new(blank());
    let bytes = article("Hello ą world");
    let old = mounted(disk.clone(), |s| install(s, 1, &bytes).unwrap());
    let base = disk.0.borrow().bytes.clone();
    for deleting in [false, true] {
        let run = |s: &mut FatStore<'_, Disk, Clock>| {
            if deleting {
                delete(s, &old, 2)
            } else {
                position(s, &old, 2, 6)
            }
        };
        let complete = Disk::new(base.clone());
        mounted(complete.clone(), |s| {
            run(s).unwrap();
        });
        let writes = complete.0.borrow().writes;
        for cut in 0..writes {
            for prefix in [0, 1, 127, 255, 511, 512] {
                let d = Disk::new(base.clone());
                {
                    let mut state = d.0.borrow_mut();
                    state.cut = Some(cut);
                    state.prefix = prefix;
                }
                mounted(d.clone(), |s| {
                    assert!(run(s).is_err());
                });
                mounted(Disk::new(d.0.borrow().bytes.clone()), |s| {
                    let r = recover(s, "a-one", 1..=2).unwrap().unwrap();
                    assert!(r.generation == 1 || r.generation == 2);
                    if r.generation == 2 {
                        assert_eq!(r.deleted, deleting);
                        if !deleting {
                            assert_eq!(r.position, 6);
                        }
                    }
                    verify(s, &r).unwrap();
                });
            }
        }
        println!(
            "{} metadata: {writes} writes, {} cuts",
            if deleting { "delete" } else { "position" },
            writes * 6
        );
    }
}
#[test]
fn large_article_stream_and_token_payload() {
    let disk = Disk::new(blank());
    mounted(disk, |s| {
        let bytes = article(&"ą".repeat(131072));
        let r = install(s, 1, &bytes).unwrap();
        verify(s, &r).unwrap();
        let mut bytes = vec![0xff; x3_proto::TOKEN_CAP];
        let h = x3_proto::payload::TokenHeader {
            bpp: 2,
            width: 528,
            height: 792,
            crc: x3_proto::payload::crc32(&bytes[16..]),
        };
        h.encode(&mut bytes).unwrap();
        let r = Record::new(
            "t-one",
            2,
            bytes.len() as u32,
            Sha256::digest(&bytes).into(),
            true,
        )
        .unwrap();
        let mut stage = Stage::begin(s, r.clone()).unwrap();
        for chunk in bytes.chunks(512) {
            stage.write(s, chunk).unwrap();
        }
        assert_eq!(stage.commit(s).unwrap(), r);
        verify(s, &r).unwrap();
    });
}
