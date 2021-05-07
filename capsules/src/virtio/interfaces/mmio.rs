//! VirtIO memory mapped device driver

use kernel::common::cells::OptionalCell;
use kernel::common::registers::{
    register_bitfields, InMemoryRegister, ReadOnly, ReadWrite, WriteOnly,
};
use kernel::common::StaticRef;

use crate::virtio::{VirtIODeviceType, VirtIODriver, VirtIOTransport, VirtQueue};

// Magic string "virt" every device has to expose
const VIRTIO_MAGIC_VALUE: [u8; 4] = [0x76, 0x69, 0x72, 0x74];

#[repr(C)]
pub struct VirtIOMMIODeviceRegisters {
    /// 0x000 Magic string "virt" for identification
    magic_value: ReadOnly<u32>,
    /// 0x004 Device version number
    device_version: ReadOnly<u32>,
    /// 0x008 VirtIO Subsystem Device ID
    device_id: ReadOnly<u32>,
    /// 0x00C VirtIO Subsystem Vendor ID
    vendor_id: ReadOnly<u32>,
    /// 0x010 Flags representing features the device supports
    device_features: ReadOnly<u32, DeviceFeatures::Register>,
    /// 0x014 Device (host) features word selection
    device_features_sel: WriteOnly<u32, DeviceFeatures::Register>,
    // 0x018 - 0x01C: reserved
    _reversed0: [u32; 2],
    /// 0x020 Flags representing features understood and activated by the driver
    driver_features: WriteOnly<u32>,
    /// 0x024 Activated (guest) features word selection
    driver_features_sel: WriteOnly<u32>,
    // 0x028 - 0x02C: reserved
    _reserved1: [u32; 2],
    /// 0x030 Virtual queue index
    queue_sel: WriteOnly<u32>,
    /// 0x034 Maximum virtual queue size
    queue_num_max: ReadOnly<u32>,
    /// 0x038 Virtual queue size
    queue_num: WriteOnly<u32>,
    // 0x03C - 0x40: reserved
    _reserved2: [u32; 2],
    /// 0x044 Virtual queue ready bit
    queue_ready: ReadWrite<u32>,
    // 0x048 - 0x04C: reserved
    _reserved3: [u32; 2],
    /// 0x050 Queue notifier
    queue_notify: WriteOnly<u32>,
    // 0x054 - 0x05C: reserved
    _reserved4: [u32; 3],
    /// 0x060 Interrupt status
    interrupt_status: ReadOnly<u32, InterruptStatus::Register>,
    /// 0x064 Interrupt acknowledge
    interrupt_ack: WriteOnly<u32, InterruptStatus::Register>,
    // 0x068 - 0x06C: reserved
    _reserved5: [u32; 2],
    /// 0x070 Device status
    device_status: ReadWrite<u32, DeviceStatus::Register>,
    // 0x074 - 0x07C: reserved
    _reserved6: [u32; 3],
    /// 0x080 - 0x084 Virtual queue's Descriptor Area 64-bit long physical address
    queue_desc_low: WriteOnly<u32>,
    queue_desc_high: WriteOnly<u32>,
    // 0x088 - 0x08C: reserved
    _reserved7: [u32; 2],
    /// 0x090 - 0x094 Virtual queue's Driver Area 64-bit long physical address
    queue_driver_low: WriteOnly<u32>,
    queue_driver_high: WriteOnly<u32>,
    // 0x098 - 0x09C: reserved
    _reserved8: [u32; 2],
    /// 0x0A0 - 0x0A4 Virtual queue's Device Area 64-bit long physical address
    queue_device_low: WriteOnly<u32>,
    queue_device_high: WriteOnly<u32>,
    // 0x0A8 - 0x0AC: reserved
    _reserved9: [u32; 21],
    /// 0x0FC Configuration atomicity value
    config_generation: ReadOnly<u32>,
    /// 0x100 - 0x19C device configuration space
    ///
    /// This is individually defined per device, with a variable
    /// size. TODO: How to address this properly? Just hand around
    /// references to this?
    config: [u32; 40],
}

register_bitfields![u32,
    DeviceStatus [
        Acknowledge OFFSET(0) NUMBITS(1) [],
        Driver OFFSET(1) NUMBITS(1) [],
        Failed OFFSET(7) NUMBITS(1) [],
        FeaturesOk OFFSET(3) NUMBITS(1) [],
        DriverOk OFFSET(2) NUMBITS(1) [],
        DeviceNeedsReset OFFSET(6) NUMBITS(1) [],
    ],
    DeviceFeatures [
        // TODO
        Dummy OFFSET(0) NUMBITS(1) [],
    ],
    InterruptStatus [
        UsedBuffer OFFSET(0) NUMBITS(1) [],
        ConfigChange OFFSET(1) NUMBITS(1) [],
    ]
];

register_bitfields![u64,
    TransportFeatures [
        RingIndirectDesc OFFSET(28) NUMBITS(1) [],
        RingEventIdx OFFSET(29) NUMBITS(1) [],
        Version1 OFFSET(32) NUMBITS(1) [],
        AccessPlatform OFFSET(33) NUMBITS(1) [],
        RingPacked OFFSET(34) NUMBITS(1) [],
        InOrder OFFSET(35) NUMBITS(1) [],
        OrderPlatform OFFSET(36) NUMBITS(1) [],
        SRIOV OFFSET(37) NUMBITS(1) []
    ]
];

pub struct VirtIOMMIODevice {
    regs: StaticRef<VirtIOMMIODeviceRegisters>,
    device_type: OptionalCell<VirtIODeviceType>,
    queues: OptionalCell<&'static [&'static dyn VirtQueue]>,
}

impl VirtIOMMIODevice {
    pub const fn new(regs: StaticRef<VirtIOMMIODeviceRegisters>) -> VirtIOMMIODevice {
        VirtIOMMIODevice {
            regs,
            device_type: OptionalCell::empty(),
            queues: OptionalCell::empty(),
        }
    }

    pub fn handle_interrupt(&self) {
        assert!(self.queues.is_some());

        let isr = self.regs.interrupt_status.extract();
        // Acknowledge all interrupts immediately so that the interrupts is deasserted
        self.regs.interrupt_ack.set(isr.get());

        if isr.is_set(InterruptStatus::UsedBuffer) {
            // Iterate over all queues, checking for new buffers in
            // the used ring
            self.queues.map(|queues| {
                for queue in queues.iter() {
                    queue.used_interrupt();
                }
            });
        }

        if isr.is_set(InterruptStatus::ConfigChange) {
            // TODO
        }
    }
}

impl VirtIOTransport for VirtIOMMIODevice {
    fn query(&self) -> Option<VirtIODeviceType> {
        // Partial initialization routine as per 4.2.3.1 MMIO-specific
        // device initialization
        //
        // This can be used to query the VirtIO transport information
        // (e.g. whether it's a supported transport and the attached
        // device)

        // Verify that we are talking to a VirtIO MMIO device...
        if self.regs.magic_value.get() != u32::from_le_bytes(VIRTIO_MAGIC_VALUE) {
            panic!("Not a VirtIO MMIO device");
        }

        // with version 2
        if self.regs.device_version.get() != 0x0002 {
            panic!(
                "Unknown VirtIO MMIO device version: {}",
                self.regs.device_version.get()
            );
        }

        // Extract the device type
        VirtIODeviceType::from_device_id(self.regs.device_id.get())
    }

    fn initialize(
        &self,
        driver: &dyn VirtIODriver,
        queues: &'static [&'static dyn VirtQueue],
    ) -> VirtIODeviceType {
        // Initialization routine as per 4.2.3.1 MMIO-specific device
        // initialization

        // Verify that we are talking to a VirtIO MMIO device...
        if self.regs.magic_value.get() != u32::from_le_bytes(VIRTIO_MAGIC_VALUE) {
            panic!("Not a VirtIO MMIO device");
        }

        // with version 2
        if self.regs.device_version.get() != 0x0002 {
            panic!("Unknown VirtIO MMIO device version");
        }

        // Extract the device type, which will later function as an
        // indicator for initialized
        let device_type =
            if let Some(dt) = VirtIODeviceType::from_device_id(self.regs.device_id.get()) {
                dt
            } else {
                // We MUST NOT access any other register in this case
                panic!("Unknown VirtIO device type or NULL");
            };

        if device_type != driver.device_type() {
            panic!("Driver does not support driving a {:?}", device_type);
        }

        // All further initialization as per 3.1 Device Initialization

        // 1. Reset the device (by writing 0x0 to the device status register)
        self.regs.device_status.set(0x0000);

        // 2. Set the ACKNOWLEDGE status bit: the guest OS has noticed
        // the device
        self.regs
            .device_status
            .modify(DeviceStatus::Acknowledge::SET);

        // 3. Set the DRIVER status bit: the guest OS knows how to
        // drive the device
        //
        // TODO: Maybe not always the case?
        self.regs.device_status.modify(DeviceStatus::Driver::SET);

        // 4. Read device feature bits, write the subset of feature
        // bits understood by OS & driver to the device
        //
        // Feature bits 0-23 are for the driver, 24-37 for the
        // transport & queue, 38 and above reserved. The caller may
        // therefore only negotiate bits 0-23 using the supplied
        // closure, others are possibly initialized by us.
        //
        // The features must be read 32 bits at a time, which are
        // chosen using DeviceFeaturesSel
        {
            // Read the virtual 64-bit register
            //
            // This is guaranteed to be consistent, the device MUST
            // NOT change its features during operation
            self.regs.device_features_sel.set(0);
            let mut device_features_reg: u64 = self.regs.device_features.get() as u64;
            self.regs.device_features_sel.set(1);
            device_features_reg |= (self.regs.device_features.get() as u64) << 32;

            // Negotiate the transport features
            let offered_transport_features: InMemoryRegister<u64, TransportFeatures::Register> =
                InMemoryRegister::new(device_features_reg);
            let selected_transport_features: InMemoryRegister<u64, TransportFeatures::Register> =
                InMemoryRegister::new(0x00000000);

            // Sanity check: Version1 must be offered AND accepted
            if !offered_transport_features.is_set(TransportFeatures::Version1) {
                panic!("Device did not offer version 1 feature!");
            } else {
                selected_transport_features.modify(TransportFeatures::Version1::SET);
            }

            // Negotiate the driver features
            let selected_features = selected_transport_features.get()
                | driver.negotiate_features(device_features_reg & 0xFFF) & 0xFFF;

            // Write the virtual 64-bit register
            self.regs.driver_features_sel.set(0);
            self.regs
                .driver_features
                .set((selected_features & 0xFFFF) as u32);
            self.regs.driver_features_sel.set(1);
            self.regs
                .driver_features
                .set((selected_features >> 32 & 0xFFFF) as u32);
        }

        // 5. Set the FEATURES_OK status bit. We MUST NOT accept new
        // feature bits after this step
        self.regs
            .device_status
            .modify(DeviceStatus::FeaturesOk::SET);

        // 6. Re-read device status to ensure that FEATURES_OK is
        // still set, otherwise the drive does not support the subset
        // of features & is unusable
        if !self.regs.device_status.is_set(DeviceStatus::FeaturesOk) {
            panic!("Device does not accept features selection");
        }

        // 7. Perform device specific setup
        //
        // A device has a number of virtqueues it supports. We try to
        // initialize all virtqueues passed in as the `queues`
        // parameter, and ignore others potentially required by the
        // device. If the `queues` parameter provides more queues than
        // the device can take, panic.
        //
        // Implementation of the algorithms of 4.2.3.2
        for (index, queue) in queues.iter().enumerate() {
            // Select the queue
            self.regs.queue_sel.set(index as u32);

            // Verify that the queue is not already in use (shouldn't
            // be, since we've just reset)
            assert!(self.regs.queue_ready.get() == 0x0000);

            // Read the maximum queue size (number of elements) from
            // QueueNumMax. If the returned value is zero, the queue
            // is not available
            let queue_num_max = self.regs.queue_num_max.get() as usize;
            if queue_num_max == 0 {
                panic!("VirtIO device does not offer queue {}", index);
            }

            // Negotiate the queue size, choosing a value fit for
            // QueueNumMax and the buffer sizes of the passed in
            // queue. This sets the negotiated value in the queue for
            // later operation.
            let queue_num = queue.negotiate_queue_size(queue_num_max);

            // Zero the queue memory
            queue.initialize(index as u32);

            // Notify the device about the queue size
            self.regs.queue_num.set(queue_num as u32);

            // Write the physical queue addresses
            let addrs = queue.physical_addresses();
            self.regs.queue_desc_low.set(addrs.descriptor_area as u32);
            self.regs
                .queue_desc_high
                .set((addrs.descriptor_area >> 32) as u32);
            self.regs.queue_driver_low.set(addrs.driver_area as u32);
            self.regs
                .queue_driver_high
                .set((addrs.driver_area >> 32) as u32);
            self.regs.queue_device_low.set(addrs.device_area as u32);
            self.regs
                .queue_device_high
                .set((addrs.device_area >> 32) as u32);

            // Set queue to ready
            self.regs.queue_ready.set(0x0001);
        }

        // Store the queue references for later usage
        self.queues.set(queues);

        // 8. Set the DRIVER_OK status bit
        self.regs.device_status.modify(DeviceStatus::DriverOk::SET);

        // The device is now "live"
        self.device_type.set(device_type);

        device_type
    }

    fn queue_notify(&self, queue_id: u32) {
        assert!(queue_id < self.queues.expect("VirtIO transport not initialized").len() as u32);
        self.regs.queue_notify.set(queue_id);
    }
}
