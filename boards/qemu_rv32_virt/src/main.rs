//! Board file for qemu-system-riscv32 "virt" machine type

#![no_std]
// Disable this attribute when documenting, as a workaround for
// https://github.com/rust-lang/rust/issues/62184.
#![cfg_attr(not(doc), no_main)]

use capsules::virtual_alarm::{MuxAlarm, VirtualMuxAlarm};
use kernel::capabilities;
use kernel::common::dynamic_deferred_call::{DynamicDeferredCall, DynamicDeferredCallClientState};
use kernel::component::Component;
use kernel::hil;
use kernel::hil::time::Alarm;
use kernel::Chip;
use kernel::Platform;
use kernel::{create_capability, debug, static_init};
use qemu_rv32_virt_chip::chip::{QemuRv32VirtChip, QemuRv32VirtDefaultPeripherals};
use rv32i::csr;

pub mod io;

pub const NUM_PROCS: usize = 4;

// Actual memory for holding the active process structures. Need an empty list
// at least.
static mut PROCESSES: [Option<&'static dyn kernel::procs::Process>; NUM_PROCS] = [None; NUM_PROCS];

// Reference to the chip for panic dumps.
static mut CHIP: Option<
    &'static QemuRv32VirtChip<
        VirtualMuxAlarm<'static, sifive::clint::Clint<'static>>,
        QemuRv32VirtDefaultPeripherals,
    >,
> = None;

// How should the kernel respond when a process faults.
const FAULT_RESPONSE: kernel::procs::FaultResponse = kernel::procs::FaultResponse::Panic;

/// Dummy buffer that causes the linker to reserve enough space for the stack.
#[no_mangle]
#[link_section = ".stack_buffer"]
pub static mut STACK_MEMORY: [u8; 0x2000] = [0; 0x2000];

// Dummy client, printing information about received packets
// struct VirtIONetDummyClient(&'static capsules::virtio::devices::virtio_net::VirtIONet<'static>);
// impl capsules::virtio::devices::virtio_net::VirtIONetClient for VirtIONetDummyClient {
//     fn packet_received(&self, id: usize, buffer: &'static mut [u8], len: usize) {
//         debug!("Packet received from virtio-net if {}, len {}.", id, len);
//         self.0.return_rx_buffer(buffer);
//     }

//     fn packet_sent(&self, _id: usize, _buffer: &'static mut [u8]) {
//         unimplemented!("VirtIONetDummyClient packet_sent");
//     }
// }

/// A structure representing this platform that holds references to all
/// capsules for this platform. We've included an alarm and console.
struct QemuRv32VirtPlatform {
    console: &'static capsules::console::Console<'static>,
    lldb: &'static capsules::low_level_debug::LowLevelDebug<
        'static,
        capsules::virtual_uart::UartDevice<'static>,
    >,
    alarm: &'static capsules::alarm::AlarmDriver<
        'static,
        VirtualMuxAlarm<'static, sifive::clint::Clint<'static>>,
    >,
    virtio_rng: Option<&'static capsules::rng::RngDriver<'static>>,
}

/// Mapping of integer syscalls to objects that implement syscalls.
impl Platform for QemuRv32VirtPlatform {
    fn with_driver<F, R>(&self, driver_num: usize, f: F) -> R
    where
        F: FnOnce(Option<&dyn kernel::Driver>) -> R,
    {
        match driver_num {
            capsules::console::DRIVER_NUM => f(Some(self.console)),
            capsules::alarm::DRIVER_NUM => f(Some(self.alarm)),
            capsules::low_level_debug::DRIVER_NUM => f(Some(self.lldb)),
            capsules::rng::DRIVER_NUM => {
                if let Some(rng_driver) = self.virtio_rng {
                    f(Some(rng_driver))
                } else {
                    f(None)
                }
            }
            _ => f(None),
        }
    }
}

/// Main function.
///
/// This function is called from the arch crate after some very basic
/// RISC-V setup and RAM initialization.
#[no_mangle]
pub unsafe fn main() {
    // ---------- BASIC INITIALIZATION -----------

    // Basic setup of the RISC-V IMAC platform
    rv32i::configure_trap_handler(rv32i::PermissionMode::Machine);

    // Acquire required capabilities
    let process_mgmt_cap = create_capability!(capabilities::ProcessManagementCapability);
    let memory_allocation_cap = create_capability!(capabilities::MemoryAllocationCapability);
    let main_loop_cap = create_capability!(capabilities::MainLoopCapability);

    // Create a board kernel instance
    let board_kernel = static_init!(kernel::Kernel, kernel::Kernel::new(&PROCESSES));

    // Some capsules require a callback from a different stack
    // frame. The dynamic deferred call infrastructure can be used to
    // request such a callback (issued from the scheduler) without
    // requiring to wire these capsule up in the chip crates.
    let dynamic_deferred_call_clients =
        static_init!([DynamicDeferredCallClientState; 2], Default::default());
    let dynamic_deferred_caller = static_init!(
        DynamicDeferredCall,
        DynamicDeferredCall::new(dynamic_deferred_call_clients)
    );
    DynamicDeferredCall::set_global_instance(dynamic_deferred_caller);

    // ---------- QEMU-SYSTEM-RISCV32 "virt" MACHINE PERIPHERALS ----------

    let peripherals = static_init!(
        QemuRv32VirtDefaultPeripherals,
        QemuRv32VirtDefaultPeripherals::new(),
    );

    // Create a shared UART channel for the console and for kernel
    // debug over the provided memory-mapped 16550-compatible
    // UART.
    let uart_mux = components::console::UartMuxComponent::new(
        &peripherals.uart0,
        115200,
        dynamic_deferred_caller,
    )
    .finalize(());

    // Use the RISC-V machine timer timesource
    let hardware_timer = static_init!(
        sifive::clint::Clint,
        sifive::clint::Clint::new(&qemu_rv32_virt_chip::clint::CLINT_BASE)
    );

    // Create a shared virtualization mux layer on top of a single hardware
    // alarm.
    let mux_alarm = static_init!(
        MuxAlarm<'static, sifive::clint::Clint>,
        MuxAlarm::new(hardware_timer)
    );
    hil::time::Alarm::set_alarm_client(hardware_timer, mux_alarm);

    // Virtual alarm for the scheduler
    let systick_virtual_alarm = static_init!(
        VirtualMuxAlarm<'static, sifive::clint::Clint>,
        VirtualMuxAlarm::new(mux_alarm)
    );

    // Virtual alarm and driver for userspace
    let virtual_alarm_user = static_init!(
        VirtualMuxAlarm<'static, sifive::clint::Clint>,
        VirtualMuxAlarm::new(mux_alarm)
    );
    let alarm = static_init!(
        capsules::alarm::AlarmDriver<'static, VirtualMuxAlarm<'static, sifive::clint::Clint>>,
        capsules::alarm::AlarmDriver::new(
            virtual_alarm_user,
            board_kernel.create_grant(&memory_allocation_cap)
        )
    );
    hil::time::Alarm::set_alarm_client(virtual_alarm_user, alarm);

    // ---------- VIRTIO PERIPHERAL DISCOVERY ----------
    //
    // This board has 8 virtio-mmio (v2 personality required!) devices
    //
    // Collect supported VirtIO peripheral indicies and initialize
    // them if they are found. If there are two instances of a
    // supported peripheral, the one on a higher-indexed VirtIO
    // transport is used.
    let (mut virtio_net_idx, mut virtio_rng_idx) = (None, None);
    for (i, virtio_device) in peripherals.virtio_mmio.iter().enumerate() {
        use capsules::virtio::{VirtIODeviceType, VirtIOTransport};
        match virtio_device.query() {
            Some(VirtIODeviceType::NetworkCard) => {
                virtio_net_idx = Some(i);
            }
            Some(VirtIODeviceType::EntropySource) => {
                virtio_rng_idx = Some(i);
            }
            _ => (),
        }
    }

    // If there is a VirtIO EntropySource present, use the appropriate
    // VirtIORng driver and expose it to userspace though the
    // RngDriver
    let virtio_rng_driver: Option<&'static capsules::rng::RngDriver<'static>> =
        if let Some(rng_idx) = virtio_rng_idx {
            use capsules::virtio::devices::virtio_rng::VirtIORng;
            use capsules::virtio::queues::split_queue::{
                SplitVirtQueue, VirtQueueAvailableRing, VirtQueueDescriptors, VirtQueueUsedRing,
            };
            use capsules::virtio::{VirtIOTransport, VirtQueue};
            use kernel::hil::rng::Rng;

            // EntropySource requires a single VirtQueue for retrieved entropy
            let descriptors = static_init!(VirtQueueDescriptors, VirtQueueDescriptors::default(),);
            let available_ring =
                static_init!(VirtQueueAvailableRing, VirtQueueAvailableRing::default(),);
            let used_ring = static_init!(VirtQueueUsedRing, VirtQueueUsedRing::default(),);
            let queue = static_init!(
                SplitVirtQueue,
                SplitVirtQueue::new(descriptors, available_ring, used_ring),
            );
            queue.set_transport(&peripherals.virtio_mmio[rng_idx]);

            // VirtIO EntropySource device driver instantiation
            let rng = static_init!(VirtIORng, VirtIORng::new(queue, dynamic_deferred_caller));
            rng.set_deferred_call_handle(
                dynamic_deferred_caller
                    .register(rng)
                    .expect("no deferred call slot available for VirtIO RNG"),
            );
            queue.set_client(rng);

            // Register the queues and driver with the transport, so
            // interrupts are routed properly
            let mmio_queues = static_init!([&'static dyn VirtQueue; 1], [queue; 1]);
            peripherals.virtio_mmio[rng_idx].initialize(rng, mmio_queues);

            // Provide an internal randomness buffer
            let rng_buffer = static_init!([u8; 64], [0; 64]);
            rng.provide_buffer(rng_buffer)
                .expect("rng: providing initial buffer failed");

            // Userspace RNG driver over the VirtIO EntropySource
            let rng_driver: &'static mut capsules::rng::RngDriver = static_init!(
                capsules::rng::RngDriver,
                capsules::rng::RngDriver::new(
                    rng,
                    board_kernel.create_grant(&memory_allocation_cap),
                ),
            );
            rng.set_client(rng_driver);

            Some(rng_driver as &'static capsules::rng::RngDriver)
        } else {
            // No VirtIO EntropySource discovered
            None
        };

    // If there is a VirtIO NetworkCard present, use the appropriate
    // VirtIONet driver. Currently this is not used, as work on the
    // userspace network driver and kernel network stack progresses.
    //
    // A template dummy driver is provided to verify basic
    // functionality of this interface.
    let _virtio_net_if: Option<&'static capsules::virtio::devices::virtio_net::VirtIONet<'static>> =
        if let Some(net_idx) = virtio_net_idx {
            use capsules::virtio::devices::virtio_net::VirtIONet;
            use capsules::virtio::queues::split_queue::{
                SplitVirtQueue, VirtQueueAvailableRing, VirtQueueDescriptors, VirtQueueUsedRing,
            };
            use capsules::virtio::{VirtIOTransport, VirtQueue};

            // A VirtIO NetworkCard requires 2 VirtQueues:
            // - a TX VirtQueue with buffers for outgoing packets
            // - a RX VirtQueue where incoming packet buffers are
            //   placed and filled by the device

            // TX VirtQueue
            let tx_descriptors =
                static_init!(VirtQueueDescriptors, VirtQueueDescriptors::default(),);
            let tx_available_ring =
                static_init!(VirtQueueAvailableRing, VirtQueueAvailableRing::default(),);
            let tx_used_ring = static_init!(VirtQueueUsedRing, VirtQueueUsedRing::default(),);
            let tx_queue = static_init!(
                SplitVirtQueue,
                SplitVirtQueue::new(tx_descriptors, tx_available_ring, tx_used_ring),
            );
            tx_queue.set_transport(&peripherals.virtio_mmio[net_idx]);

            // RX VirtQueue
            let rx_descriptors =
                static_init!(VirtQueueDescriptors, VirtQueueDescriptors::default(),);
            let rx_available_ring =
                static_init!(VirtQueueAvailableRing, VirtQueueAvailableRing::default(),);
            let rx_used_ring = static_init!(VirtQueueUsedRing, VirtQueueUsedRing::default(),);
            let rx_queue = static_init!(
                SplitVirtQueue,
                SplitVirtQueue::new(rx_descriptors, rx_available_ring, rx_used_ring),
            );
            rx_queue.set_transport(&peripherals.virtio_mmio[net_idx]);

            // Incoming and outgoing packets are prefixed by a 12-byte
            // VirtIO specific header
            let tx_header_buf = static_init!([u8; 12], [0; 12]);
            let rx_header_buf = static_init!([u8; 12], [0; 12]);

            // Currently, provide a single receive buffer to write
            // incoming packets into
            let rx_buffer = static_init!([u8; 1526], [0; 1526]);

            // Instantiate the VirtIONet (NetworkCard) driver and set
            // the queues
            let net = static_init!(
                VirtIONet<'static>,
                VirtIONet::new(
                    0,
                    tx_queue,
                    tx_header_buf,
                    rx_queue,
                    rx_header_buf,
                    rx_buffer,
                ),
            );
            tx_queue.set_client(net);
            rx_queue.set_client(net);

            // Register the queues and driver with the transport, so
            // interrupts are routed properly
            let mmio_queues = static_init!([&'static dyn VirtQueue; 2], [rx_queue, tx_queue]);
            peripherals.virtio_mmio[net_idx].initialize(net, mmio_queues);

            // TODO: As soon as an appropriate driver is available,
            // this should return a driver instead of an interface
            // reference

            // Dummy client, printing information about received packets
            // let dummy_client = static_init!(VirtIONetDummyClient, VirtIONetDummyClient(net));
            // net.set_client(dummy_client);

            // Place a buffer in the receive VirtQueue, so the device
            // can write the first packet to it
            // net.initialize_rx();

            Some(net as &'static VirtIONet)
        } else {
            // No VirtIO NetworkCard discovered
            None
        };

    // ---------- INITIALIZE CHIP, ENABLE INTERRUPTS ---------

    let chip = static_init!(
        QemuRv32VirtChip<
            VirtualMuxAlarm<'static, sifive::clint::Clint>,
            QemuRv32VirtDefaultPeripherals,
        >,
        QemuRv32VirtChip::new(systick_virtual_alarm, peripherals, hardware_timer),
    );
    systick_virtual_alarm.set_alarm_client(chip.scheduler_timer());
    CHIP = Some(chip);

    // Need to enable all interrupts for Tock Kernel
    chip.enable_plic_interrupts();

    // enable interrupts globally
    csr::CSR
        .mie
        .modify(csr::mie::mie::mext::SET + csr::mie::mie::msoft::SET + csr::mie::mie::mtimer::SET);
    csr::CSR.mstatus.modify(csr::mstatus::mstatus::mie::SET);

    // ---------- FINAL SYSTEM INITIALIZATION ----------

    // Setup the console.
    let console = components::console::ConsoleComponent::new(board_kernel, uart_mux).finalize(());
    // Create the debugger object that handles calls to `debug!()`.
    components::debug_writer::DebugWriterComponent::new(uart_mux).finalize(());

    let lldb = components::lldb::LowLevelDebugComponent::new(board_kernel, uart_mux).finalize(());

    debug!("QEMU RISC-V 32-bit \"virt\" machine, initialization complete.");
    debug!("Entering main loop.");

    /// These symbols are defined in the linker script.
    extern "C" {
        /// Beginning of the ROM region containing app images.
        static _sapps: u8;
        /// End of the ROM region containing app images.
        static _eapps: u8;
        /// Beginning of the RAM region for app memory.
        static mut _sappmem: u8;
        /// End of the RAM region for app memory.
        static _eappmem: u8;
    }

    let platform = QemuRv32VirtPlatform {
        console,
        alarm,
        lldb,
        virtio_rng: virtio_rng_driver,
    };

    // ---------- PROCESS LOADING, SCHEDULER LOOP ----------

    kernel::procs::load_processes(
        board_kernel,
        chip,
        core::slice::from_raw_parts(
            &_sapps as *const u8,
            &_eapps as *const u8 as usize - &_sapps as *const u8 as usize,
        ),
        core::slice::from_raw_parts_mut(
            &mut _sappmem as *mut u8,
            &_eappmem as *const u8 as usize - &_sappmem as *const u8 as usize,
        ),
        &mut PROCESSES,
        FAULT_RESPONSE,
        &process_mgmt_cap,
    )
    .unwrap_or_else(|err| {
        debug!("Error loading processes!");
        debug!("{:?}", err);
    });

    let scheduler = components::sched::cooperative::CooperativeComponent::new(&PROCESSES)
        .finalize(components::coop_component_helper!(NUM_PROCS));
    board_kernel.kernel_loop(
        &platform,
        chip,
        None::<&kernel::ipc::IPC<NUM_PROCS>>,
        scheduler,
        &main_loop_cap,
    );
}
