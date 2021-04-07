//! Interface for reading, writing, and erasing flash storage pages.
//!
//! Operates on single pages. The page size is set by the associated type
//! `page`. Here is an example of a page type and implementation of this trait:
//!
//! ```rust
//! #![feature(min_const_generics)]
//! use core::ops::{Index, IndexMut};
//! use kernel::hil::flash;
//! use kernel::ReturnCode;
//!
//! // Size in bytes
//! const PAGE_SIZE: u32 = 1024;
//!
//! struct NewChipStruct;
//!
//! impl<'a, C> flash::HasClient<'a, C> for NewChipStruct {
//!     fn set_client(&'a self, client: &'a C) { unimplemented!() }
//! }
//!
//! impl<const W: usize, const E: usize> flash::Flash<W, E> for NewChipStruct {
//!     fn read(
//!         &self,
//!         address: u64,
//!         length: usize,
//!         buf: &'static mut [u8],
//!     ) -> Result<(), (ReturnCode, &'static mut [u8])> {
//!        unimplemented!()
//!     }
//!
//!     fn write(
//!         &self,
//!         address: u64,
//!         buf: &'static mut[u8],
//!     ) -> Result<(), (ReturnCode, &'static mut [u8])> {
//!         unimplemented!()
//!     }
//!
//!     fn erase(&self, address: u64, length: usize) -> Result<(), ReturnCode> {
//!         unimplemented!()
//!     }
//! }
//! ```
//!
//! A user of this flash interface might look like:
//!
//! ```rust
//! #![feature(min_const_generics)]
//! use kernel::common::cells::TakeCell;
//! use kernel::hil::flash;
//! use kernel::ReturnCode;
//!
//! pub struct FlashUser<'a, F: flash::Flash<W, E> + 'static, const W: usize, const E: usize> {
//!     driver: &'a F,
//!     buffer: TakeCell<'static, [u8; W]>,
//! }
//!
//! impl<'a, F: flash::Flash<W, E>, const W: usize, const E: usize> FlashUser<'a, F, W, E> {
//!     pub fn new(driver: &'a F, buffer: &'static mut [u8; W]) -> FlashUser<'a, F, W, E> {
//!         FlashUser {
//!             driver: driver,
//!             buffer: TakeCell::new(buffer),
//!         }
//!     }
//! }
//!
//! impl<'a, F: flash::Flash<W, E>, const W: usize, const E: usize> flash::Client<W, E> for FlashUser<'a, F, W, E> {
//!     fn read_complete(&self, read_buffer: &'static mut [u8], ret: Result<(), ReturnCode>) {}
//!     fn write_complete(&self, write_buffer: &'static mut [u8], ret: Result<(), ReturnCode>) { }
//!     fn erase_complete(&self, ret: Result<(), ReturnCode>) {}
//! }
//! ```

use crate::returncode::ReturnCode;

pub trait HasClient<'a, C> {
    /// Set the client for this flash peripheral. The client will be called
    /// when operations complete.
    fn set_client(&'a self, client: &'a C);
}

/// Flash errors returned in the callbacks.
/// Depreated
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum LegacyError {
    /// Success.
    CommandComplete,

    /// An error occurred during the flash operation.
    FlashError,
}

/// A page of writable persistent flash memory.
/// Depreated
pub trait LegacyFlash {
    /// Type of a single flash page for the given implementation.
    type Page: AsMut<[u8]> + Default;

    /// Read a page of flash into the buffer.
    fn read_page(
        &self,
        page_number: usize,
        buf: &'static mut Self::Page,
    ) -> Result<(), (ReturnCode, &'static mut Self::Page)>;

    /// Write a page of flash from the buffer.
    fn write_page(
        &self,
        page_number: usize,
        buf: &'static mut Self::Page,
    ) -> Result<(), (ReturnCode, &'static mut Self::Page)>;

    /// Erase a page of flash by setting every byte to 0xFF.
    fn erase_page(&self, page_number: usize) -> ReturnCode;
}

/// Implement `Client` to receive callbacks from `Flash`.
/// Depreated
pub trait LegacyClient<F: LegacyFlash> {
    /// Flash read complete.
    fn read_complete(&self, read_buffer: &'static mut F::Page, error: LegacyError);

    /// Flash write complete.
    fn write_complete(&self, write_buffer: &'static mut F::Page, error: LegacyError);

    /// Flash erase complete.
    fn erase_complete(&self, error: LegacyError);
}

/// A page of writeable persistent flash memory.
///
/// `W`: Should be the minimum number of bytes that can be written
///      in an operation.
/// `E`: Should be the minimum number of bytes that can be erased
///      in an operation.
pub trait Flash<const W: usize, const E: usize> {
    /// Read data from flash into a buffer.
    ///
    /// This function will read data stored in flash at `address` and
    /// `length` into the buffer `buf`.
    /// `address` is calculated as an offset from the start of the flash
    /// region.
    ///
    /// On success returns nothing
    /// On failure returns a `ReturnCode` and the buffer passed in.
    /// If `ReturnCode::ENOSUPPORT` is returned then `read_page()`
    // should be used instead.
    fn read(
        &self,
        address: u64,
        length: usize,
        buf: &'static mut [u8],
    ) -> Result<(), (ReturnCode, &'static mut [u8])>;

    /// Write data from a buffer to flash.
    ///
    /// This function will write the buffer `buf` to the `address` specified
    /// in flash.
    ///
    /// `address` must be aligned to `W`.
    /// The length of `buf` must be aligned to `W`.
    ///
    /// This function will not erase the page first. The user of this function
    /// must ensure that a page is erased before writing.
    /// Writes to flash can only turn a `1` to a `0`. To change a `0` to a `1`
    /// the region must be erased.
    ///
    /// Note that some hardware only allows a limited number of writes before
    /// an erase. If that is the case the implementation MUST return an error
    /// `ReturnCode::ENOMEM` when this happens, even if the hardware silently
    /// ignores the write.
    ///
    /// On success returns nothing
    /// On failure returns a `ReturnCode` and the buffer passed in.
    fn write(
        &self,
        address: u64,
        buf: &'static mut [u8],
    ) -> Result<(), (ReturnCode, &'static mut [u8])>;

    /// Erase a page/pages of flash, setting every byte to 0xFF.
    ///
    /// This will erase all pages starting from `address` for the `length`.
    /// `address` and `length must allign with `E`.
    ///
    /// On success returns nothing
    /// On failure returns a `ReturnCode`.
    fn erase(&self, address: u64, length: usize) -> Result<(), ReturnCode>;
}

/// Implement `Client` to receive callbacks from `Flash`.
pub trait Client<const W: usize, const E: usize> {
    /// Flash read complete.
    ///
    /// This will be called when the read operation is complete.
    /// On success `ret` will be nothing.
    /// On error `ret` will contain a `ReturnCode`
    fn read_complete(&self, read_buffer: &'static mut [u8], ret: Result<(), ReturnCode>);

    /// Flash write complete.
    ///
    /// This will be called when the write operation is complete.
    /// On success `ret` will be nothing.
    /// On error `ret` will contain a `ReturnCode`
    fn write_complete(&self, write_buffer: &'static mut [u8], ret: Result<(), ReturnCode>);

    /// Flash erase complete.
    ///
    /// This will be called when the erase operation is complete.
    /// On success `ret` will be nothing.
    /// On error `ret` will contain a `ReturnCode`
    fn erase_complete(&self, ret: Result<(), ReturnCode>);
}
