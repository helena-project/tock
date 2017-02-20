#![feature(asm,concat_idents,const_fn)]
#![no_std]

extern crate kernel;

extern "C" {
    pub fn init();
}

mod peripheral_registers;
mod peripheral_interrupts;
mod nvic;

pub mod chip;
pub mod gpio;
pub mod rtc;
pub mod timer;
pub mod clock;
pub mod uart;
pub mod pinmux;
pub mod aes_ccm;
pub use chip::NRF51;
pub mod temperature;
