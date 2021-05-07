use core::fmt::Write;
use core::panic::PanicInfo;
use core::str;

use kernel::debug;
use kernel::debug::IoWrite;
use rv32i;

use crate::CHIP;
use crate::PROCESSES;

struct Writer {}

static mut WRITER: Writer = Writer {};

impl Write for Writer {
    fn write_str(&mut self, s: &str) -> ::core::fmt::Result {
        self.write(s.as_bytes());
        Ok(())
    }
}

impl IoWrite for Writer {
    fn write(&mut self, buf: &[u8]) {
        let uart = qemu_rv32_virt_chip::uart::Uart16550::new(qemu_rv32_virt_chip::uart::UART0_BASE);
        uart.transmit_sync(buf);
    }
}

/// Panic handler.
#[cfg(not(test))]
#[no_mangle]
#[panic_handler]
pub unsafe extern "C" fn panic_fmt(pi: &PanicInfo) -> ! {
    let writer = &mut WRITER;

    debug::panic_print::<_, _>(writer, pi, &rv32i::support::nop, &PROCESSES, &CHIP);

    // The system is no longer in a well-defined state, so loop
    // endlessly.
    //
    // If possible we might want to exit the LiteX simulation in the
    // future instead.
    loop {}
}
