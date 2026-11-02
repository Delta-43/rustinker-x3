//! Optional embedded-sdmmc adapter. Use a dedicated directory on the SD card.
//! BlockDevice writes must finish before returning; real card power-loss
//! behaviour still needs the author-run tests. No media is opened by this module.
use crate::{
    Error,
    storage::{Catalog, FileKey, Store},
};
use embedded_sdmmc::{BlockDevice, Mode, RawDirectory, TimeSource, VolumeManager};
pub struct FatStore<'a, D: BlockDevice, T: TimeSource> {
    pub manager: &'a VolumeManager<D, T>,
    pub directory: RawDirectory,
}
impl<D: BlockDevice, T: TimeSource> Catalog for FatStore<'_, D, T> {
    fn next_generation(&mut self) -> Result<u32, Error> {
        let mut max = 0;
        self.manager
            .iterate_dir(self.directory, |entry| {
                if matches!(entry.name.extension(), b"DAT" | b"MET")
                    && entry.name.base_name().len() == 8
                    && let Ok(s) = core::str::from_utf8(entry.name.base_name())
                    && let Ok(n) = u32::from_str_radix(s, 16)
                {
                    max = max.max(n);
                }
                core::ops::ControlFlow::Continue(())
            })
            .map_err(|_| Error::Storage)?;
        max.checked_add(1).ok_or(Error::Storage)
    }
    fn generations(&mut self, after: u32, out: &mut [u32]) -> Result<usize, Error> {
        self.list(b"MET", after, out)
    }
    fn payload_generations(&mut self, after: u32, out: &mut [u32]) -> Result<usize, Error> {
        self.list(b"DAT", after, out)
    }
    fn remove(&mut self, key: FileKey) -> Result<(), Error> {
        let name = key.name();
        let name = core::str::from_utf8(&name).map_err(|_| Error::Invalid)?;
        match self.manager.delete_entry_in_dir(self.directory, name) {
            Ok(()) | Err(embedded_sdmmc::Error::NotFound) => Ok(()),
            Err(_) => Err(Error::Storage),
        }
    }
}
impl<D: BlockDevice, T: TimeSource> FatStore<'_, D, T> {
    fn list(&mut self, ext: &[u8; 3], after: u32, out: &mut [u32]) -> Result<usize, Error> {
        let mut n = 0;
        self.manager
            .iterate_dir(self.directory, |entry| {
                if entry.name.extension() == ext
                    && entry.name.base_name().len() == 8
                    && let Ok(s) = core::str::from_utf8(entry.name.base_name())
                    && let Ok(id) = u32::from_str_radix(s, 16)
                    && id > after
                {
                    if n < out.len() {
                        out[n] = id;
                        n += 1;
                        out[..n].sort_unstable();
                    } else if let Some(last) = out.last_mut()
                        && id < *last
                    {
                        *last = id;
                        out.sort_unstable();
                    }
                }
                core::ops::ControlFlow::Continue(())
            })
            .map_err(|_| Error::Storage)?;
        Ok(n)
    }
}
impl<D: BlockDevice, T: TimeSource> Store for FatStore<'_, D, T> {
    fn create(&mut self, key: FileKey) -> Result<(), Error> {
        let name = key.name();
        let name = core::str::from_utf8(&name).map_err(|_| Error::Invalid)?;
        let f = self
            .manager
            .open_file_in_dir(self.directory, name, Mode::ReadWriteCreate)
            .map_err(|_| Error::Storage)?;
        self.manager.close_file(f).map_err(|_| Error::Storage)
    }
    fn append(&mut self, key: FileKey, bytes: &[u8]) -> Result<(), Error> {
        let name = key.name();
        let name = core::str::from_utf8(&name).map_err(|_| Error::Invalid)?;
        let f = self
            .manager
            .open_file_in_dir(self.directory, name, Mode::ReadWriteAppend)
            .map_err(|_| Error::Storage)?;
        let result = self.manager.write(f, bytes).map_err(|_| Error::Storage);
        let close = self.manager.close_file(f).map_err(|_| Error::Storage);
        result.and(close)
    }
    fn sync(&mut self, _key: FileKey) -> Result<(), Error> {
        Ok(())
    } // append closes and flushes each write
    fn len(&mut self, key: FileKey) -> Result<usize, Error> {
        let name = key.name();
        let entry = self
            .manager
            .find_directory_entry(
                self.directory,
                core::str::from_utf8(&name).map_err(|_| Error::Invalid)?,
            )
            .map_err(|_| Error::Storage)?;
        Ok(entry.size as usize)
    }
    fn read(&mut self, key: FileKey, offset: usize, out: &mut [u8]) -> Result<usize, Error> {
        let name = key.name();
        let f = self
            .manager
            .open_file_in_dir(
                self.directory,
                core::str::from_utf8(&name).map_err(|_| Error::Invalid)?,
                Mode::ReadOnly,
            )
            .map_err(|_| Error::Storage)?;
        let result = self
            .manager
            .file_seek_from_start(f, u32::try_from(offset).map_err(|_| Error::TooLarge)?)
            .and_then(|()| self.manager.read(f, out))
            .map_err(|_| Error::Storage);
        let close = self.manager.close_file(f).map_err(|_| Error::Storage);
        match (result, close) {
            (Ok(n), Ok(())) => Ok(n),
            (Err(e), _) | (_, Err(e)) => Err(e),
        }
    }
}
