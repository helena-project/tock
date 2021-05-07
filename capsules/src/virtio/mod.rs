pub mod devices;
pub mod interfaces;
pub mod queues;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[repr(u32)]
pub enum VirtIODeviceType {
    NetworkCard = 1,
    BlockDevice = 2,
    Console = 3,
    EntropySource = 4,
    TraditionalMemoryBallooning = 5,
    IoMemory = 6,
    RPMSG = 7,
    SCSIHost = 8,
    Transport9P = 9,
    Mac80211Wlan = 10,
    RPROCSerial = 11,
    VirtIOCAIF = 12,
    MemoryBalloon = 13,
    GPUDevice = 14,
    TimerClockDevice = 15,
    InputDevice = 16,
    SocketDevice = 17,
    CryptoDevice = 18,
    SignalDistributionModule = 19,
    PstoreDevice = 20,
    IOMMUDevice = 21,
    MemoryDevice = 22,
}

impl VirtIODeviceType {
    pub fn from_device_id(id: u32) -> Option<VirtIODeviceType> {
        use VirtIODeviceType as DT;

        match id {
            1 => Some(DT::NetworkCard),
            2 => Some(DT::BlockDevice),
            3 => Some(DT::Console),
            4 => Some(DT::EntropySource),
            5 => Some(DT::TraditionalMemoryBallooning),
            6 => Some(DT::IoMemory),
            7 => Some(DT::RPMSG),
            8 => Some(DT::SCSIHost),
            9 => Some(DT::Transport9P),
            10 => Some(DT::Mac80211Wlan),
            11 => Some(DT::RPROCSerial),
            12 => Some(DT::VirtIOCAIF),
            13 => Some(DT::MemoryBalloon),
            14 => Some(DT::GPUDevice),
            15 => Some(DT::TimerClockDevice),
            16 => Some(DT::InputDevice),
            17 => Some(DT::SocketDevice),
            18 => Some(DT::CryptoDevice),
            19 => Some(DT::SignalDistributionModule),
            20 => Some(DT::PstoreDevice),
            21 => Some(DT::IOMMUDevice),
            22 => Some(DT::MemoryDevice),
            _ => None,
        }
    }

    pub fn to_device_id(device_type: VirtIODeviceType) -> u32 {
        device_type as u32
    }
}

pub trait VirtIOTransport {
    fn query(&self) -> Option<VirtIODeviceType>;
    fn initialize(
        &self,
        driver: &dyn VirtIODriver,
        queues: &'static [&'static dyn VirtQueue],
    ) -> VirtIODeviceType;
    fn queue_notify(&self, queue_id: u32);
}

pub struct VirtQueueAddresses {
    pub descriptor_area: u64,
    pub driver_area: u64,
    pub device_area: u64,
}

pub trait VirtQueue {
    fn physical_addresses(&self) -> VirtQueueAddresses;
    fn negotiate_queue_size(&self, device_max_elements: usize) -> usize;
    fn initialize(&self, queue_number: u32);
    fn used_interrupt(&self);
    fn negotiated_queue_size(&self) -> usize;
}

pub trait VirtIODriver {
    fn negotiate_features(&self, offered_features: u64) -> u64;
    fn device_type(&self) -> VirtIODeviceType;
}
