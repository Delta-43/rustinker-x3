//! Allocation-free host and device logic. No hardware drivers.
#![no_std]
#![forbid(unsafe_code)]

pub mod battery;
pub mod bitmap;
pub mod buttons;
pub mod cache;
pub mod dhcp;
pub mod dns;
#[cfg(feature = "fat")]
pub mod fat;
pub mod font;
pub mod gui;
pub mod library;
pub mod otadata;
pub mod panel;
pub mod reader;
pub mod receive;
pub mod rtc;
pub mod screens;
pub mod setup;
pub mod storage;
pub mod text;
pub mod token;
pub mod window;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Invalid,
    TooLarge,
    BufferTooSmall,
    Storage,
    Corrupt,
}

/// Implementations must serialise access to the display bus.
pub trait Display {
    fn show(&mut self, packed: &[u8]) -> Result<(), Error>;
}
pub trait Clock {
    fn unix_seconds(&self) -> u64;
}
pub trait Buttons {
    fn pressed(&mut self) -> u8;
}
pub trait Radio {
    fn set_receiving(&mut self, enabled: bool) -> Result<(), Error>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    pub token_limit: u16,
    pub receive_seconds: u16,
    pub full_refresh_pages: u8,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            token_limit: 20,
            receive_seconds: 180,
            full_refresh_pages: 10,
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<(), Error> {
        if !(1..=100).contains(&self.token_limit)
            || !(30..=600).contains(&self.receive_seconds)
            || !(1..=50).contains(&self.full_refresh_pages)
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }
}
