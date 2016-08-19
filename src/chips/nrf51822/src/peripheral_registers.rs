// Automatically generated by tools/nRF51_codegen.py, but modified by alevy
use common::VolatileCell;
use PinCnf;

pub const UART0_BASE: usize = 0x40002000;
pub struct UART0 {
    pub tasks_startrx: VolatileCell<u32>,
    pub tasks_stoprx: VolatileCell<u32>,
    pub tasks_starttx: VolatileCell<u32>,
    pub tasks_stoptx: VolatileCell<u32>,
    _reserved1: [u32; 3],
    pub tasks_suspend: VolatileCell<u32>,
    _reserved2: [u32; 56],
    pub events_cts: VolatileCell<u32>,
    pub events_ncts: VolatileCell<u32>,
    pub events_rxdrdy: VolatileCell<u32>,
    _reserved3: [u32; 4],
    pub events_txdrdy: VolatileCell<u32>,
    _reserved4: [u32; 1],
    pub events_error: VolatileCell<u32>,
    _reserved5: [u32; 7],
    pub events_rxto: VolatileCell<u32>,
    _reserved6: [u32; 46],
    pub shorts: VolatileCell<u32>,
    _reserved7: [u32; 64],
    pub intenset: VolatileCell<u32>,
    pub intenclr: VolatileCell<u32>,
    _reserved8: [u32; 93],
    pub errorsrc: VolatileCell<u32>,
    _reserved9: [u32; 31],
    pub enable: VolatileCell<u32>,
    _reserved10: [u32; 1],
    pub pselrts: VolatileCell<PinCnf>,
    pub pseltxd: VolatileCell<PinCnf>,
    pub pselcts: VolatileCell<PinCnf>,
    pub pselrxd: VolatileCell<PinCnf>,
    pub rxd: VolatileCell<u32>,
    pub txd: VolatileCell<u32>,
    _reserved11: [u32; 1],
    pub baudrate: VolatileCell<u32>,
    _reserved12: [u32; 17],
    pub config: VolatileCell<u32>,
    _reserved13: [u32; 675],
    pub power: VolatileCell<u32>,
}

pub const GPIO_BASE: usize = 0x50000000;
pub struct GPIO {
    _reserved1: [u32; 321],
    pub out: VolatileCell<u32>,
    pub outset: VolatileCell<u32>,
    pub outclr: VolatileCell<u32>,
    pub in_: VolatileCell<u32>,
    pub dir: VolatileCell<u32>,
    pub dirset: VolatileCell<u32>,
    pub dirclr: VolatileCell<u32>,
    _reserved2: [u32; 120],
    pub pin_cnf: [VolatileCell<u32>; 32],
}
