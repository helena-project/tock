use core::cell::Cell;

use kernel::common::cells::OptionalCell;
use kernel::ErrorCode;

use crate::virtio::queues::split_queue::{SplitVirtQueue, SplitVirtQueueClient, VirtQueueBuffer};
use crate::virtio::{VirtIODeviceType, VirtIODriver};

pub struct VirtIONet<'a> {
    id: Cell<usize>,
    rxqueue: &'a SplitVirtQueue<'static, 'static>,
    txqueue: &'a SplitVirtQueue<'static, 'static>,
    tx_header: OptionalCell<&'static mut [u8]>,
    rx_header: OptionalCell<&'static mut [u8]>,
    rx_buffer: OptionalCell<&'static mut [u8]>,
    client: OptionalCell<&'a dyn VirtIONetClient>,
}

impl<'a> VirtIONet<'a> {
    pub fn new(
        id: usize,
        txqueue: &'a SplitVirtQueue<'static, 'static>,
        tx_header: &'static mut [u8],
        rxqueue: &'a SplitVirtQueue<'static, 'static>,
        rx_header: &'static mut [u8],
        rx_buffer: &'static mut [u8],
    ) -> VirtIONet<'a> {
        assert!(tx_header.len() == 12);

        VirtIONet {
            id: Cell::new(id),
            rxqueue,
            txqueue,
            tx_header: OptionalCell::new(tx_header),
            client: OptionalCell::empty(),
            rx_header: OptionalCell::new(rx_header),
            rx_buffer: OptionalCell::new(rx_buffer),
        }
    }

    pub fn id(&self) -> usize {
        self.id.get()
    }

    pub fn set_client(&self, client: &'a dyn VirtIONetClient) {
        self.client.set(client);
    }

    pub fn initialize_rx(&self) {
        // To start operation, put the receive buffers into the device initially
        let rx_buffer = self.rx_buffer.take().unwrap();
        let rx_buffer_len = rx_buffer.len();

        let mut buffer_chain = [
            Some(VirtQueueBuffer {
                buf: self.rx_header.take().unwrap(),
                len: 12,
                device_writable: true,
            }),
            Some(VirtQueueBuffer {
                buf: rx_buffer,
                len: rx_buffer_len,
                device_writable: true,
            }),
        ];

        self.rxqueue.enable_used_callbacks();
        self.rxqueue
            .provide_buffer_chain(&mut buffer_chain)
            .unwrap();
    }

    pub fn return_rx_buffer(&self, buf: &'static mut [u8]) {
        assert!(self.rx_buffer.is_none());
        assert!(self.rx_header.is_some());
        self.rx_buffer.replace(buf);
        self.initialize_rx();
    }

    pub fn send_packet(
        &self,
        packet: &'static mut [u8],
        packet_len: usize,
    ) -> Result<(), (ErrorCode, &'static mut [u8])> {
        // Try to get a hold of the header buffer
        //
        // Otherwise, the device is currently busy transmissing a buffer
        //
        // TODO: Implement simultaneous transmissions
        let mut packet_buf = Some(VirtQueueBuffer {
            buf: packet,
            len: packet_len,
            device_writable: false,
        });

        let header_buf = self
            .tx_header
            .take()
            .ok_or(ErrorCode::BUSY)
            .map_err(|err| (err, packet_buf.take().unwrap().buf))?;

        // Write the header
        //
        // TODO: Can this be done using a struct of registers?
        header_buf[0] = 0; // flags -> we don't want checksumming
        header_buf[1] = 0; // gso -> no checksumming or fragmentation
        header_buf[2] = 0; // hdr_len_low
        header_buf[3] = 0; // hdr_len_high
        header_buf[4] = 0; // gso_size
        header_buf[5] = 0; // gso_size
        header_buf[6] = 0; // csum_start
        header_buf[7] = 0; // csum_start
        header_buf[8] = 0; // csum_offset
        header_buf[9] = 0; // csum_offsetb
        header_buf[10] = 0; // num_buffers
        header_buf[11] = 0; // num_buffers

        let mut buffer_chain = [
            Some(VirtQueueBuffer {
                buf: header_buf,
                len: 12,
                device_writable: false,
            }),
            packet_buf.take(),
        ];

        self.txqueue.enable_used_callbacks();
        self.txqueue
            .provide_buffer_chain(&mut buffer_chain)
            .map_err(move |err| (err, buffer_chain[1].take().unwrap().buf))?;

        Ok(())
    }
}

impl<'a> SplitVirtQueueClient<'static> for VirtIONet<'a> {
    fn buffer_chain_ready(
        &self,
        queue_number: u32,
        buffer_chain: &mut [Option<VirtQueueBuffer<'static>>],
        bytes_used: usize,
    ) {
        if queue_number == self.rxqueue.queue_number() {
            // Received a packet

            let rx_header = buffer_chain[0].take().expect("No header buffer").buf;
            // TODO: do something with the header
            self.rx_header.replace(rx_header);

            let rx_buffer = buffer_chain[1].take().expect("No rx content buffer").buf;
            self.client.map(move |client| {
                client.packet_received(self.id.get(), rx_buffer, bytes_used - 12)
            });
        } else if queue_number == self.txqueue.queue_number() {
            // Sent a packet

            let header_buf = buffer_chain[0].take().expect("No header buffer").buf;
            assert!(header_buf.len() == 12);
            self.tx_header.replace(header_buf);

            let packet_buf = buffer_chain[1].take().expect("No packet buffer").buf;
            self.client
                .map(move |client| client.packet_sent(self.id.get(), packet_buf));
        } else {
            panic!("Callback from unknown queue");
        }
    }
}

impl<'a> VirtIODriver for VirtIONet<'a> {
    fn negotiate_features(&self, offered_features: u64) -> u64 {
        let mut negotiated_features: u64 = 0;

        if offered_features & (1 << 5) != 0 {
            // VIRTIO_NET_F_MAC offered
            //
            // accept
            negotiated_features |= 1 << 5;
        } else {
            panic!("Missing NET_F_MAC");
        }

        // if offered_features & (1 << 15) != 0 {
        //     // VIRTIO_NET_F_MRG_RXBUF
        //     //
        //     // accept
        //     negotiated_features |= 1 << 15;
        // } else {
        //     panic!("Missing NET_F_MRG_RXBUF");
        // }

        // Ignore everything else
        negotiated_features
    }

    fn device_type(&self) -> VirtIODeviceType {
        VirtIODeviceType::NetworkCard
    }
}

pub trait VirtIONetClient {
    fn packet_sent(&self, id: usize, buffer: &'static mut [u8]);
    fn packet_received(&self, id: usize, buffer: &'static mut [u8], len: usize);
}
