//! GPIO driver.

use core::ops::{Index, IndexMut};
use kernel::common::cells::OptionalCell;
use kernel::common::registers::interfaces::{ReadWriteable, Readable, Writeable};
use kernel::common::registers::{
    register_bitfields, register_structs, Field, ReadWrite, WriteOnly,
};
use kernel::common::StaticRef;
use kernel::hil::gpio;

pub const GPIO_BASE: StaticRef<GpioRegisters> =
    unsafe { StaticRef::new(0x6000_4000 as *const GpioRegisters) };

register_structs! {
    pub GpioRegisters {
        (0x0 => bt_select: ReadWrite<u32>),
        (0x4 => gpio_out: ReadWrite<u32, pins::Register>),
        (0x8 => gpio_out_w1ts: WriteOnly<u32, pins::Register>),
        (0xC => gpio_out_w1tc: WriteOnly<u32, pins::Register>),
        (0x10 => _reserved0),
        (0x1C => sdio_select: ReadWrite<u32>),
        (0x20 => enable: ReadWrite<u32, pins::Register>),
        (0x24 => enable_w1ts: ReadWrite<u32, pins::Register>),
        (0x28 => enable_w1tc: ReadWrite<u32, pins::Register>),
        (0x2C => _reserved1),
        (0x38 => strap: ReadWrite<u32>),
        (0x3C => gpio_in: ReadWrite<u32, pins::Register>),
        (0x40 => _reserved2),
        (0x44 => status: ReadWrite<u32, pins::Register>),
        (0x48 => status_w1ts: ReadWrite<u32, pins::Register>),
        (0x4C => status_w1tc: ReadWrite<u32, pins::Register>),
        (0x50 => _reserved3),
        (0x5C => pcpu_int: ReadWrite<u32>),
        (0x60 => pcpu_nmi_int: ReadWrite<u32>),
        (0x64 => cpusdio_int: ReadWrite<u32>),
        (0x68 => _reserved4),
        (0x74 => pin: [ReadWrite<u32, PIN::Register>; 26]),
        (0xDC => _reserved5),
        (0x14C => status_next: ReadWrite<u32>),
        (0x150 => _reserved6),
        (0x154 => func_in_sel_cfg: [ReadWrite<u32>; 128]),
        (0x354 => _reserved7),
        (0x554 => func_out_sel_cfg: [ReadWrite<u32>; 26]),
        (0x5BC => _reserved8),
        (0x62C => clock_gate: ReadWrite<u32>),
        (0x630 => _reserved9),
        (0x6FC => date: ReadWrite<u32>),
        (0x700 => @END),
    }
}

register_bitfields![u32,
    pub pins [
        pin0 0,
        pin1 1,
        pin2 2,
        pin3 3,
        pin4 4,
        pin5 5,
        pin6 6,
        pin7 7,
        pin8 8,
        pin9 9,
        pin10 10,
        pin11 11,
        pin12 12,
        pin13 13,
        pin14 14,
        pin15 15,
        pin16 16,
        pin17 17,
        pin18 18,
        pin19 19,
        pin20 20,
        pin21 21,
        pin22 22,
        pin23 23,
        pin24 24,
        pin25 25
    ],
    MASK_HALF [
        DATA OFFSET(0) NUMBITS(16) [],
        MASK OFFSET(16) NUMBITS(16) [],
    ],
    PIN [
        INT_ENA OFFSET(13) NUMBITS(5) [
            Disabled = 0,
            Enable = 1,
            NMI = 2,
        ],
        CONFIG OFFSET(11) NUMBITS(2) [],
        WAKEUP_ENABLE OFFSET(10) NUMBITS(1) [],
        INT_TYPE OFFSET(7) NUMBITS(3) [
            DISABLE = 0,
            POSEDGE = 1,
            NEGEDGE = 2,
            ANYEDGE = 3,
            LOW_LEVEL = 4,
            HIGH_LEVEL = 5,
        ],
        SYNC1_BYPASS OFFSET(3) NUMBITS(2) [],
        PAD_DRIVER OFFSET(2) NUMBITS(1) [],
        SYNC2_BYPASS OFFSET(0) NUMBITS(2) [],
    ],
];

pub struct GpioPin<'a> {
    registers: StaticRef<GpioRegisters>,
    pin: Field<u32, pins::Register>,
    client: OptionalCell<&'a dyn gpio::Client>,
}

impl<'a> GpioPin<'a> {
    pub const fn new(
        gpio_base: StaticRef<GpioRegisters>,
        pin: Field<u32, pins::Register>,
    ) -> GpioPin<'a> {
        GpioPin {
            registers: gpio_base,
            pin,
            client: OptionalCell::empty(),
        }
    }

    pub fn handle_interrupt(&self) {
        unimplemented!()
    }
}

impl gpio::Configure for GpioPin<'_> {
    fn configuration(&self) -> gpio::Configuration {
        if self.registers.enable.is_set(self.pin) {
            gpio::Configuration::Input
        } else {
            gpio::Configuration::InputOutput
        }
    }

    fn set_floating_state(&self, _mode: gpio::FloatingState) {
        unimplemented!()
    }

    fn floating_state(&self) -> gpio::FloatingState {
        unimplemented!()
    }

    fn deactivate_to_low_power(&self) {
        self.disable_input();
        self.disable_output();
    }

    fn make_output(&self) -> gpio::Configuration {
        self.registers
            .enable_w1ts
            .set(self.pin.mask << self.pin.shift);
        gpio::Configuration::Output
    }

    fn disable_output(&self) -> gpio::Configuration {
        self.registers
            .enable_w1tc
            .set(self.pin.mask << self.pin.shift);
        gpio::Configuration::Input
    }

    fn make_input(&self) -> gpio::Configuration {
        self.configuration()
    }

    fn disable_input(&self) -> gpio::Configuration {
        /* We can't do this from the GPIO contorller.
         * It does look like the IO Mux is capable of this
         * though.
         */
        gpio::Configuration::Input
    }
}

impl gpio::Input for GpioPin<'_> {
    fn read(&self) -> bool {
        self.registers.gpio_in.is_set(self.pin)
    }
}

impl gpio::Output for GpioPin<'_> {
    fn toggle(&self) -> bool {
        let old_state = self.registers.gpio_out.is_set(self.pin);
        if old_state {
            self.clear();
        } else {
            self.set();
        }
        self.registers.gpio_out.is_set(self.pin)
    }

    fn set(&self) {
        self.registers
            .gpio_out_w1ts
            .set(self.pin.mask << self.pin.shift);
    }

    fn clear(&self) {
        self.registers
            .gpio_out_w1tc
            .set(self.pin.mask << self.pin.shift);
    }
}

impl<'a> gpio::Interrupt<'a> for GpioPin<'a> {
    fn set_client(&self, client: &'a dyn gpio::Client) {
        self.client.set(client);
    }

    fn enable_interrupts(&self, mode: gpio::InterruptEdge) {
        self.registers.pin[self.pin.shift].modify(PIN::INT_ENA::Enable);

        match mode {
            gpio::InterruptEdge::RisingEdge => {
                self.registers.pin[self.pin.shift].modify(PIN::INT_TYPE::POSEDGE);
            }
            gpio::InterruptEdge::FallingEdge => {
                self.registers.pin[self.pin.shift].modify(PIN::INT_TYPE::NEGEDGE);
            }
            gpio::InterruptEdge::EitherEdge => {
                self.registers.pin[self.pin.shift].modify(PIN::INT_TYPE::ANYEDGE);
            }
        }

        self.registers.pin[self.pin.shift].modify(PIN::WAKEUP_ENABLE::SET);
    }

    fn disable_interrupts(&self) {
        self.registers.pin[self.pin.shift].modify(PIN::INT_ENA::Disabled);
    }

    fn is_pending(&self) -> bool {
        self.registers.status.is_set(self.pin)
    }
}

pub struct Port<'a> {
    pins: [GpioPin<'a>; 17],
}

impl<'a> Port<'a> {
    pub const fn new() -> Self {
        Self {
            pins: [
                GpioPin::new(GPIO_BASE, pins::pin0),
                GpioPin::new(GPIO_BASE, pins::pin1),
                GpioPin::new(GPIO_BASE, pins::pin2),
                GpioPin::new(GPIO_BASE, pins::pin3),
                GpioPin::new(GPIO_BASE, pins::pin4),
                GpioPin::new(GPIO_BASE, pins::pin5),
                GpioPin::new(GPIO_BASE, pins::pin6),
                GpioPin::new(GPIO_BASE, pins::pin7),
                GpioPin::new(GPIO_BASE, pins::pin8),
                GpioPin::new(GPIO_BASE, pins::pin9),
                GpioPin::new(GPIO_BASE, pins::pin10),
                GpioPin::new(GPIO_BASE, pins::pin11),
                GpioPin::new(GPIO_BASE, pins::pin12),
                GpioPin::new(GPIO_BASE, pins::pin13),
                GpioPin::new(GPIO_BASE, pins::pin14),
                GpioPin::new(GPIO_BASE, pins::pin15),
                GpioPin::new(GPIO_BASE, pins::pin16),
            ],
        }
    }
}

impl<'a> Index<usize> for Port<'a> {
    type Output = GpioPin<'a>;

    fn index(&self, index: usize) -> &GpioPin<'a> {
        &self.pins[index]
    }
}

impl<'a> IndexMut<usize> for Port<'a> {
    fn index_mut(&mut self, index: usize) -> &mut GpioPin<'a> {
        &mut self.pins[index]
    }
}
