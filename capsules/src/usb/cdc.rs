//! CDC

use core::cell::Cell;
use core::cmp;

use super::descriptors::{
    self, Buffer64, CsInterfaceDescriptor, EndpointAddress, EndpointDescriptor,
    InterfaceDescriptor, TransferDirection,
};
use super::usbc_client_ctrl::ClientCtrl;

use kernel::common::cells::OptionalCell;
use kernel::common::cells::TakeCell;
use kernel::common::cells::VolatileCell;
use kernel::debug;
use kernel::hil;
use kernel::hil::uart;
use kernel::hil::usb::TransferType;
use kernel::ReturnCode;

const VENDOR_ID: u16 = 0x6668;
const PRODUCT_ID: u16 = 0xabce;

/// Identifying number for the endpoint when transferring data from us to the
/// host.
const ENDPOINT_IN_NUM: usize = 2;
/// Identifying number for the endpoint when transferring data from the host to
/// us.
const ENDPOINT_OUT_NUM: usize = 3;

static LANGUAGES: &'static [u16; 1] = &[
    0x0409, // English (United States)
];

static STRINGS: &'static [&'static str] = &[
    "aXYZ Corp.",      // Manufacturer
    "aThe Zorpinator", // Product
    "aSerial No. 5",   // Serial number
];

pub const MAX_CTRL_PACKET_SIZE_SAM4L: u8 = 8;
pub const MAX_CTRL_PACKET_SIZE_NRF52840: u8 = 64;

const N_ENDPOINTS: usize = 3;

pub struct Cdc<'a, C: 'a> {
    client_ctrl: ClientCtrl<'a, 'static, C>,

    // An eight-byte buffer for each endpoint
    buffers: [Buffer64; N_ENDPOINTS],

    /// A holder reference for the TX buffer we are transmitting from.
    tx_buffer: TakeCell<'static, [u8]>,
    /// The number of bytes the client has asked us to send. We track this so we
    /// can pass it back to the client when the transmission has finished.
    tx_len: Cell<usize>,
    /// How many more bytes we need to transmit. This is used in our TX state
    /// machine.
    tx_remaining: Cell<usize>,
    /// Where in the `tx_buffer` we need to start sending from when we continue.
    tx_offset: Cell<usize>,
    /// The TX client to use when transmissions finish.
    tx_client: OptionalCell<&'a dyn uart::TransmitClient>,
}

impl<'a, C: hil::usb::UsbController<'a>> Cdc<'a, C> {
    pub fn new(controller: &'a C, max_ctrl_packet_size: u8) -> Self {
        let interfaces: &mut [InterfaceDescriptor] = &mut [
            InterfaceDescriptor {
                interface_number: 0,
                interface_class: 0x02,    // CDC communication
                interface_subclass: 0x02, // abstract control model (ACM)
                interface_protocol: 0x01, // V.25ter (AT commands)
                ..InterfaceDescriptor::default()
            },
            InterfaceDescriptor {
                interface_number: 1,
                interface_class: 0x0a,    // CDC data
                interface_subclass: 0x00, // none
                interface_protocol: 0x00, // none
                ..InterfaceDescriptor::default()
            },
        ];

        let cdc_descriptors: &mut [CsInterfaceDescriptor] = &mut [
            CsInterfaceDescriptor {
                subtype: descriptors::CsInterfaceDescriptorSubType::Header,
                field1: 0x10, // CDC
                field2: 0x11, // CDC
            },
            CsInterfaceDescriptor {
                subtype: descriptors::CsInterfaceDescriptorSubType::CallManagement,
                field1: 0x00,
                field2: 0x01,
            },
            CsInterfaceDescriptor {
                subtype: descriptors::CsInterfaceDescriptorSubType::AbstractControlManagement,
                field1: 0x06,
                field2: 0x00, // unused
            },
            // make the length work for now......
            CsInterfaceDescriptor {
                subtype: descriptors::CsInterfaceDescriptorSubType::Union,
                field1: 0x00, // Interface 0
                field2: 0x01, // Interface 1
            },
        ];

        let endpoints: &[&[EndpointDescriptor]] = &[
            &[EndpointDescriptor {
                endpoint_address: EndpointAddress::new_const(4, TransferDirection::DeviceToHost),
                transfer_type: TransferType::Interrupt,
                max_packet_size: 8,
                interval: 100,
            }],
            &[
                EndpointDescriptor {
                    endpoint_address: EndpointAddress::new_const(
                        2,
                        TransferDirection::DeviceToHost,
                    ),
                    transfer_type: TransferType::Bulk,
                    max_packet_size: 64,
                    interval: 100,
                },
                EndpointDescriptor {
                    endpoint_address: EndpointAddress::new_const(
                        3,
                        TransferDirection::HostToDevice,
                    ),
                    transfer_type: TransferType::Bulk,
                    max_packet_size: 64,
                    interval: 100,
                },
            ],
        ];

        let (device_descriptor_buffer, other_descriptor_buffer) =
            descriptors::create_descriptor_buffers(
                descriptors::DeviceDescriptor {
                    vendor_id: VENDOR_ID,
                    product_id: PRODUCT_ID,
                    manufacturer_string: 1,
                    product_string: 2,
                    serial_number_string: 3,
                    class: 0x2, // Class: CDC
                    max_packet_size_ep0: max_ctrl_packet_size,
                    ..descriptors::DeviceDescriptor::default()
                },
                descriptors::ConfigurationDescriptor {
                    ..descriptors::ConfigurationDescriptor::default()
                },
                interfaces,
                endpoints,
                None, // No HID descriptor
                Some(cdc_descriptors),
            );

        Cdc {
            client_ctrl: ClientCtrl::new(
                controller,
                device_descriptor_buffer,
                other_descriptor_buffer,
                None, // No HID descriptor
                None, // No report descriptor
                LANGUAGES,
                STRINGS,
            ),
            buffers: [
                Buffer64::default(),
                Buffer64::default(),
                Buffer64::default(),
            ],
            tx_buffer: TakeCell::empty(),
            tx_len: Cell::new(0),
            tx_remaining: Cell::new(0),
            tx_offset: Cell::new(0),
            tx_client: OptionalCell::empty(),
        }
    }

    #[inline]
    fn controller(&self) -> &'a C {
        self.client_ctrl.controller()
    }

    #[inline]
    fn buffer(&'a self, i: usize) -> &'a [VolatileCell<u8>; 64] {
        &self.buffers[i - 1].buf
    }
}

impl<'a, C: hil::usb::UsbController<'a>> hil::usb::Client<'a> for Cdc<'a, C> {
    fn enable(&'a self) {
        // Set up the default control endpoint
        self.client_ctrl.enable();

        // Setup buffers for IN and OUT data transfer.
        self.controller()
            .endpoint_set_in_buffer(ENDPOINT_IN_NUM, self.buffer(ENDPOINT_IN_NUM));
        self.controller()
            .endpoint_in_enable(TransferType::Bulk, ENDPOINT_IN_NUM);

        self.controller()
            .endpoint_set_out_buffer(ENDPOINT_OUT_NUM, self.buffer(ENDPOINT_OUT_NUM));
        self.controller()
            .endpoint_out_enable(TransferType::Bulk, ENDPOINT_OUT_NUM);
    }

    fn attach(&'a self) {
        self.client_ctrl.attach();
    }

    fn bus_reset(&'a self) {
        // Should the client initiate reconfiguration here?
        // For now, the hardware layer does it.

        debug!("Bus reset");
    }

    /// Handle a Control Setup transaction
    fn ctrl_setup(&'a self, endpoint: usize) -> hil::usb::CtrlSetupResult {
        self.client_ctrl.ctrl_setup(endpoint)
    }

    /// Handle a Control In transaction
    fn ctrl_in(&'a self, endpoint: usize) -> hil::usb::CtrlInResult {
        self.client_ctrl.ctrl_in(endpoint)
    }

    /// Handle a Control Out transaction
    fn ctrl_out(&'a self, endpoint: usize, packet_bytes: u32) -> hil::usb::CtrlOutResult {
        self.client_ctrl.ctrl_out(endpoint, packet_bytes)
    }

    fn ctrl_status(&'a self, endpoint: usize) {
        self.client_ctrl.ctrl_status(endpoint)
    }

    /// Handle the completion of a Control transfer
    fn ctrl_status_complete(&'a self, endpoint: usize) {
        self.client_ctrl.ctrl_status_complete(endpoint)
    }

    /// Handle a Bulk/Interrupt IN transaction.
    ///
    /// This is called when we can send data to the host. It should get called
    /// when we tell the controller we want to resume the IN endpoint (meaning
    /// we know we have data to send) and afterwards until we return
    /// `hil::usb::InResult::Delay` from this function. That means we can use
    /// this as a callback to mean that the transmission finished by waiting
    /// until this function is called when we don't have anything left to send.
    fn packet_in(&'a self, transfer_type: TransferType, endpoint: usize) -> hil::usb::InResult {
        match transfer_type {
            TransferType::Interrupt => {
                debug!("interrupt_in({}) not implemented", endpoint);
                hil::usb::InResult::Error
            }
            TransferType::Bulk => {
                self.tx_buffer
                    .take()
                    .map_or(hil::usb::InResult::Delay, |tx_buf| {
                        // Check if we have any bytes to send.
                        let remaining = self.tx_remaining.get();
                        if remaining > 0 {
                            // We do, so we go ahead and send those.

                            // Get packet that we have shared with the underlying
                            // USB stack to copy the tx into.
                            let packet = self.buffer(endpoint);

                            // Calculate how much more we can send.
                            let to_send = cmp::min(packet.len(), remaining);

                            // Copy from the TX buffer to the outgoing USB packet.
                            let offset = self.tx_offset.get();
                            for i in 0..to_send {
                                packet[i].set(tx_buf[offset + i]);
                            }

                            // Update our state on how much more there is to send.
                            self.tx_remaining.set(remaining - to_send);
                            self.tx_offset.set(offset + to_send);

                            // Put the TX buffer back so we can keep sending from it.
                            self.tx_buffer.replace(tx_buf);

                            // Return that we have data to send.
                            hil::usb::InResult::Packet(to_send)
                        } else {
                            // We don't have anything to send, so that means we are
                            // ok to signal the callback.

                            // Signal the callback and pass back the TX buffer.
                            self.tx_client.map(move |tx_client| {
                                tx_client.transmitted_buffer(
                                    tx_buf,
                                    self.tx_len.get(),
                                    ReturnCode::SUCCESS,
                                )
                            });

                            // Return that we have nothing else to do to the USB
                            // driver.
                            hil::usb::InResult::Delay
                        }
                    })

                // if self.last_char.is_some() {

                //     let packet = self.buffer(endpoint);

                //     packet[0].set(self.last_char.unwrap_or(66));

                //     self.last_char.clear();

                //     // self.controller().endpoint_resume_out(3);

                //     hil::usb::InResult::Packet(1)

                // } else {
                //     hil::usb::InResult::Delay
                // }

                // // Write a packet into the endpoint buffer
                // let packet_bytes = self.echo_len.get();
                // if packet_bytes > 0 {
                //     // Copy the entire echo buffer into the packet
                //     let packet = self.buffer(endpoint);
                //     for i in 0..packet_bytes {
                //         packet[i].set(self.echo_buf[i].get());
                //     }
                //     self.echo_len.set(0);

                //     // We can receive more now
                //     self.alert_empty();

                //     hil::usb::InResult::Packet(packet_bytes)
                // } else {
                //     // Nothing to send
                //     hil::usb::InResult::Delay
                // }
            }
            TransferType::Control | TransferType::Isochronous => unreachable!(),
        }
    }

    /// Handle a Bulk/Interrupt OUT transaction
    fn packet_out(
        &'a self,
        transfer_type: TransferType,
        endpoint: usize,
        packet_bytes: u32,
    ) -> hil::usb::OutResult {
        debug!("packet out {} {}", endpoint, packet_bytes);
        match transfer_type {
            TransferType::Interrupt => {
                debug!("interrupt_out({}) not implemented", endpoint);
                hil::usb::OutResult::Error
            }
            TransferType::Bulk => {
                // Consume a packet from the endpoint buffer
                // let new_len = packet_bytes as usize;
                // let current_len = self.echo_len.get();
                // let total_len = current_len + new_len as usize;

                // let packet = self.buffer(endpoint);

                // debug!("got {}", packet[0].get());

                // self.last_char.set(packet[0].get());

                // self.controller().endpoint_resume_in(2);

                // if total_len > self.echo_buf.len() {
                //     // The packet won't fit in our little buffer.  We'll have
                //     // to wait until it is drained
                //     self.delayed_out.set(true);
                //     hil::usb::OutResult::Delay
                // } else if new_len > 0 {
                //     // Copy the packet into our echo buffer
                //     let packet = self.buffer(endpoint);
                //     for i in 0..new_len {
                //         self.echo_buf[current_len + i].set(packet[i].get());
                //     }
                //     self.echo_len.set(total_len);

                //     // We can start sending again
                //     self.alert_full();
                //     hil::usb::OutResult::Ok
                // } else {
                //     debug!("Ignoring zero-length OUT packet");
                //     hil::usb::OutResult::Ok
                // }

                hil::usb::OutResult::Ok
            }
            TransferType::Control | TransferType::Isochronous => unreachable!(),
        }
    }

    fn packet_transmitted(&'a self, _endpoint: usize) {
        // Nothing to do.
    }
}

impl<'a, C: hil::usb::UsbController<'a>> uart::Configure for Cdc<'a, C> {
    fn configure(&self, parameters: uart::Parameters) -> ReturnCode {
        ReturnCode::SUCCESS
    }
}

impl<'a, C: hil::usb::UsbController<'a>> uart::Transmit<'a> for Cdc<'a, C> {
    fn set_transmit_client(&self, client: &'a dyn uart::TransmitClient) {
        self.tx_client.set(client);
    }

    fn transmit_buffer(
        &self,
        tx_buffer: &'static mut [u8],
        tx_len: usize,
    ) -> (ReturnCode, Option<&'static mut [u8]>) {
        if self.tx_buffer.is_some() {
            // We are already handling a transmission, we cannot queue another
            // request.
            (ReturnCode::EBUSY, Some(tx_buffer))
        } else {
            if tx_len > tx_buffer.len() {
                // Can't send more bytes than will fit in the buffer.
                return (ReturnCode::ESIZE, Some(tx_buffer));
            }

            // Ok, we can handle this transmission. Initialize all of our state
            // for our TX state machine.
            self.tx_remaining.set(tx_len);
            self.tx_len.set(tx_len);
            self.tx_offset.set(0);
            self.tx_buffer.replace(tx_buffer);

            // Then signal to the lower layer that we are ready to do a TX by
            // putting data in the IN endpoint.
            self.controller().endpoint_resume_in(ENDPOINT_IN_NUM);

            (ReturnCode::SUCCESS, None)
        }
    }

    fn transmit_abort(&self) -> ReturnCode {
        ReturnCode::FAIL
    }

    fn transmit_word(&self, _word: u32) -> ReturnCode {
        ReturnCode::FAIL
    }
}

impl<'a, C: hil::usb::UsbController<'a>> uart::Receive<'a> for Cdc<'a, C> {
    fn set_receive_client(&self, client: &'a dyn uart::ReceiveClient) {

    }

    fn receive_buffer(
        &self,
        rx_buffer: &'static mut [u8],
        rx_len: usize,
    ) -> (ReturnCode, Option<&'static mut [u8]>) {
        // if rx_len > rx_buffer.len() {
        //     return (ReturnCode::ESIZE, Some(rx_buffer));
        // }
        // let usart = &USARTRegManager::new(&self);

        // // enable RX
        // self.enable_rx(usart);
        // self.enable_rx_error_interrupts(usart);
        // self.usart_rx_state.set(USARTStateRX::DMA_Receiving);
        // // set up dma transfer and start reception
        // if let Some(dma) = self.rx_dma.get() {
        //     dma.enable();
        //     self.rx_len.set(rx_len);
        //     dma.do_transfer(self.rx_dma_peripheral, rx_buffer, rx_len);
            (ReturnCode::SUCCESS, None)
        // } else {
        //     (ReturnCode::EOFF, Some(rx_buffer))
        // }
    }

    fn receive_abort(&self) -> ReturnCode {
        ReturnCode::FAIL
    }

    fn receive_word(&self) -> ReturnCode {
        ReturnCode::FAIL
    }
}

impl<'a, C: hil::usb::UsbController<'a>> uart::Uart<'a> for Cdc<'a, C> {}
impl<'a, C: hil::usb::UsbController<'a>> uart::UartData<'a> for Cdc<'a, C> {}