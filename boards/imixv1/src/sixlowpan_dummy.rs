//! A dummy sixlowpan/IP sender

use capsules::net::lowpan;
use capsules::net::lowpan::{ContextStore, Context, LoWPAN};
use capsules::net::ip::{IP6Header, MacAddr, IPAddr, ip6_nh};
use capsules::net::util;
use core::mem;
use kernel::hil::radio;

pub struct DummyStore<'a> {
    context0: Context<'a>,
}

impl<'a> DummyStore<'a> {
    pub fn new(context0: Context<'a>) -> DummyStore<'a> {
        DummyStore { context0: context0 }
    }
}

impl<'a> ContextStore<'a> for DummyStore<'a> {
    // These methods should also include context 0 (the mesh-local prefix) as
    // one of the possible options

    fn get_context_from_addr(&self, ip_addr: IPAddr) -> Option<Context<'a>> {
        if util::matches_prefix(&ip_addr,
                                self.context0.prefix,
                                self.context0.prefix_len) {
            Some(self.context0)
        } else {
            None
        }
    }

    fn get_context_from_id(&self, ctx_id: u8) -> Option<Context<'a>> {
        if ctx_id == 0 {
            Some(self.context0)
        } else {
            None
        }
    }

    fn get_context_from_prefix(&self, prefix: &[u8], prefix_len: u8) -> Option<Context<'a>> {
        if prefix_len == self.context0.prefix_len &&
           util::matches_prefix(prefix, self.context0.prefix, prefix_len) {
            Some(self.context0)
        } else {
            None
        }
    }
}

pub const MLP: [u8; 8] = [0xc0, 0xc1, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7];
pub const LLP: [u8; 8] = [0x80, 0x81, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87];
pub const SRC_ADDR: IPAddr = [0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07,
                              0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f];
pub const DST_ADDR: IPAddr = [0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27,
                              0x28, 0x29, 0x2a, 0x2b, 0x2c, 0x2d, 0x2e, 0x2f];
pub const SRC_MAC_ADDR: MacAddr = MacAddr::LongAddr([0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17]);
pub const DST_MAC_ADDR: MacAddr = MacAddr::LongAddr([0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f]);

// duplicated from lowpan::iphc to avoid making iphc public
pub const MAC_BASE: [u8; 8] = [0, 0, 0, 0xff, 0xfe, 0, 0, 0];

pub const IP6_HDR_SIZE: usize = 40;
pub const PAYLOAD_LEN: usize = 10;
pub static mut RF233_BUF: [u8; radio::MAX_BUF_SIZE] = [0 as u8; radio::MAX_BUF_SIZE];

#[derive(Copy,Clone,Debug)]
enum TrafficFlow {
    Inline = 0b00,
    Traffic = 0b01,
    Flow = 0b10,
    TrafficFlow = 0b11,
}

#[derive(Copy,Clone,Debug)]
enum SAC {
    Inline,
    LLP64,
    LLP16,
    LLPIID,
    Unspecified,
    Ctx64,
    Ctx16,
    CtxIID
}

#[derive(Copy,Clone,Debug)]
enum DAC {
    Inline,
    LLP64,
    LLP16,
    LLPIID,
    Ctx64,
    Ctx16,
    CtxIID,
    McastInline,
    Mcast48,
    Mcast32,
    Mcast8,
    McastCtx,
}

pub fn sixlowpan_dummy_test<R: radio::Radio>(radio: &R) {
    // Change TF compression
    ipv6_packet_test(radio, TrafficFlow::Inline, 255,
                     SAC::Inline, DAC::Inline);
    ipv6_packet_test(radio, TrafficFlow::Traffic, 255,
                     SAC::Inline, DAC::Inline);
    ipv6_packet_test(radio, TrafficFlow::Flow, 255,
                     SAC::Inline, DAC::Inline);
    ipv6_packet_test(radio, TrafficFlow::TrafficFlow, 255,
                     SAC::Inline, DAC::Inline);

    // Change HL compression
    ipv6_packet_test(radio, TrafficFlow::TrafficFlow, 255,
                     SAC::Inline, DAC::Inline);
    ipv6_packet_test(radio, TrafficFlow::TrafficFlow, 64,
                     SAC::Inline, DAC::Inline);
    ipv6_packet_test(radio, TrafficFlow::TrafficFlow, 1,
                     SAC::Inline, DAC::Inline);
    ipv6_packet_test(radio, TrafficFlow::TrafficFlow, 42,
                     SAC::Inline, DAC::Inline);

    // Change source compression
    ipv6_packet_test(radio, TrafficFlow::TrafficFlow, 42,
                     SAC::Inline, DAC::Inline);
    ipv6_packet_test(radio, TrafficFlow::TrafficFlow, 42,
                     SAC::LLP64, DAC::Inline);
    ipv6_packet_test(radio, TrafficFlow::TrafficFlow, 42,
                     SAC::LLP16, DAC::Inline);
    ipv6_packet_test(radio, TrafficFlow::TrafficFlow, 42,
                     SAC::LLPIID, DAC::Inline);
    ipv6_packet_test(radio, TrafficFlow::TrafficFlow, 42,
                     SAC::Unspecified, DAC::Inline);
    ipv6_packet_test(radio, TrafficFlow::TrafficFlow, 42,
                     SAC::Ctx64, DAC::Inline);
    ipv6_packet_test(radio, TrafficFlow::TrafficFlow, 42,
                     SAC::Ctx16, DAC::Inline);
    ipv6_packet_test(radio, TrafficFlow::TrafficFlow, 42,
                     SAC::CtxIID, DAC::Inline);

    // Change dest compression
    ipv6_packet_test(radio, TrafficFlow::TrafficFlow, 42,
                     SAC::CtxIID, DAC::Inline);
    ipv6_packet_test(radio, TrafficFlow::TrafficFlow, 42,
                     SAC::CtxIID, DAC::LLP64);
    ipv6_packet_test(radio, TrafficFlow::TrafficFlow, 42,
                     SAC::CtxIID, DAC::LLP16);
    ipv6_packet_test(radio, TrafficFlow::TrafficFlow, 42,
                     SAC::CtxIID, DAC::LLPIID);
    ipv6_packet_test(radio, TrafficFlow::TrafficFlow, 42,
                     SAC::CtxIID, DAC::Ctx64);
    ipv6_packet_test(radio, TrafficFlow::TrafficFlow, 42,
                     SAC::CtxIID, DAC::Ctx16);
    ipv6_packet_test(radio, TrafficFlow::TrafficFlow, 42,
                     SAC::CtxIID, DAC::CtxIID);
    ipv6_packet_test(radio, TrafficFlow::TrafficFlow, 42,
                     SAC::CtxIID, DAC::McastInline);
    ipv6_packet_test(radio, TrafficFlow::TrafficFlow, 42,
                     SAC::CtxIID, DAC::Mcast48);
    ipv6_packet_test(radio, TrafficFlow::TrafficFlow, 42,
                     SAC::CtxIID, DAC::Mcast32);
    ipv6_packet_test(radio, TrafficFlow::TrafficFlow, 42,
                     SAC::CtxIID, DAC::Mcast8);
    ipv6_packet_test(radio, TrafficFlow::TrafficFlow, 42,
                     SAC::CtxIID, DAC::McastCtx);

    loop {}
}

fn ipv6_packet_test<R: radio::Radio>(radio: &R,
                                     tf: TrafficFlow,
                                     hop_limit: u8,
                                     sac: SAC,
                                     dac: DAC) {
    let mut ip6_datagram = [0 as u8; IP6_HDR_SIZE + PAYLOAD_LEN];
    {
        let mut payload = &mut ip6_datagram[IP6_HDR_SIZE..];
        for i in 0..PAYLOAD_LEN {
            payload[i] = i as u8;
        }
    }
    {
        let mut ip6_header: &mut IP6Header = unsafe {
            mem::transmute(ip6_datagram.as_mut_ptr())
        };
        *ip6_header = IP6Header::new();
        ip6_header.set_payload_len(PAYLOAD_LEN as u16);

        ip6_header.set_ecn(0b01);
        if (tf as u8) & (TrafficFlow::Traffic as u8) != 0 {
            ip6_header.set_dscp(0b000000);
        } else {
            ip6_header.set_dscp(0b101010);
        }

        if (tf as u8) & (TrafficFlow::Flow as u8) != 0 {
            ip6_header.set_flow_label(0);
        } else {
            ip6_header.set_flow_label(0xABCDE);
        }

        ip6_header.set_next_header(ip6_nh::NO_NEXT);

        ip6_header.set_hop_limit(hop_limit);

        let mut src_addr: IPAddr = [0; 16];
        match sac {
            SAC::Inline => { src_addr.copy_from_slice(&SRC_ADDR); }
            SAC::LLP64 => {
                // LLP::xxxx:xxxx:xxxx:xxxx
                src_addr[0..8].copy_from_slice(&LLP);
                src_addr[8..16].copy_from_slice(&SRC_ADDR[8..16]);
            }
            SAC::LLP16 => {
                // LLP::ff:fe00:xxxx
                src_addr[0..8].copy_from_slice(&LLP);
                // Distinct from compute_iid because the U/L bit is not flipped
                src_addr[8..16].copy_from_slice(&MAC_BASE);
                src_addr[15..16].copy_from_slice(&SRC_ADDR[15..16]);
            }
            SAC::LLPIID => {
                // LLP::IID
                src_addr[0..8].copy_from_slice(&LLP);
                src_addr[8..16].copy_from_slice(
                    &lowpan::compute_iid(&SRC_MAC_ADDR));
            }
            SAC::Unspecified => {}
            SAC::Ctx64 => {
                // MLP::xxxx:xxxx:xxxx:xxxx
                src_addr[0..8].copy_from_slice(&MLP);
            }
            SAC::Ctx16 => {
                // MLP::ff:fe00:xxxx
                src_addr[0..8].copy_from_slice(&MLP);
                // Distinct from compute_iid because the U/L bit is not flipped
                src_addr[8..16].copy_from_slice(&MAC_BASE);
                src_addr[15..16].copy_from_slice(&SRC_ADDR[15..16]);
            }
            SAC::CtxIID => {
                // MLP::IID
                src_addr[0..8].copy_from_slice(&MLP);
                src_addr[8..16].copy_from_slice(
                    &lowpan::compute_iid(&SRC_MAC_ADDR));
            }
        }
        ip6_header.set_src_addr(src_addr);

        let mut dst_addr: IPAddr = [0; 16];
        match dac {
            DAC::Inline => { dst_addr.copy_from_slice(&DST_ADDR); }
            DAC::LLP64 => {
                // LLP::xxxx:xxxx:xxxx:xxxx
                dst_addr[0..8].copy_from_slice(&LLP);
                dst_addr[8..16].copy_from_slice(&DST_ADDR[8..16]);
            }
            DAC::LLP16 => {
                // LLP::ff:fe00:xxxx
                dst_addr[0..8].copy_from_slice(&LLP);
                // Distinct from compute_iid because the U/L bit is not flipped
                dst_addr[8..16].copy_from_slice(&MAC_BASE);
                dst_addr[15..16].copy_from_slice(&DST_ADDR[15..16]);
            }
            DAC::LLPIID => {
                // LLP::IID
                dst_addr[0..8].copy_from_slice(&LLP);
                dst_addr[8..16].copy_from_slice(
                    &lowpan::compute_iid(&DST_MAC_ADDR));
            }
            DAC::Ctx64 => {
                // MLP::xxxx:xxxx:xxxx:xxxx
                dst_addr[0..8].copy_from_slice(&MLP);
            }
            DAC::Ctx16 => {
                // MLP::ff:fe00:xxxx
                dst_addr[0..8].copy_from_slice(&MLP);
                // Distinct from compute_iid because the U/L bit is not flipped
                dst_addr[8..16].copy_from_slice(&MAC_BASE);
                dst_addr[15..16].copy_from_slice(&DST_ADDR[15..16]);
            }
            DAC::CtxIID => {
                // MLP::IID
                dst_addr[0..8].copy_from_slice(&MLP);
                dst_addr[8..16].copy_from_slice(
                    &lowpan::compute_iid(&DST_MAC_ADDR));
            }
            DAC::McastInline => {
                // first byte is ff, that's all we know
                dst_addr[0..16].copy_from_slice(&DST_ADDR);
                dst_addr[0] = 0xff;
            }
            DAC::Mcast48 => {
                // ffXX::00XX:XXXX:XXXX
                dst_addr[0] = 0xff;
                dst_addr[1] = DST_ADDR[1];
                dst_addr[11..16].copy_from_slice(&DST_ADDR[11..16]);
            }
            DAC::Mcast32 => {
                // ffXX::00XX:XXXX
                dst_addr[0] = 0xff;
                dst_addr[1] = DST_ADDR[1];
                dst_addr[13..16].copy_from_slice(&DST_ADDR[13..16]);
            }
            DAC::Mcast8 => {
                // ff02::00XX
                dst_addr[0] = 0xff;
                dst_addr[1] = DST_ADDR[1];
                dst_addr[15] = DST_ADDR[15];
            }
            DAC::McastCtx => {
                // ffXX:XX + plen + pfx64 + XXXX:XXXX
                dst_addr[0] = 0xff;
                dst_addr[1] = DST_ADDR[1];
                dst_addr[2] = DST_ADDR[2];
                dst_addr[3] = 64 as u8;
                dst_addr[4..12].copy_from_slice(&MLP);
                dst_addr[12..16].copy_from_slice(&DST_ADDR[12..16]);
            }
        }
        ip6_header.set_dst_addr(dst_addr);
    }
    debug!("Packet with tf={:?} hl={} sac={:?} dac={:?}",
           tf, hop_limit, sac, dac);
    unsafe {
        send_ipv6_packet(radio, &MLP, SRC_MAC_ADDR, DST_MAC_ADDR,
                         &ip6_datagram[0..IP6_HDR_SIZE + PAYLOAD_LEN]);
    }
}

unsafe fn send_ipv6_packet<R: radio::Radio>(radio: &R,
                                            mesh_local_prefix: &[u8],
                                            src_mac_addr: MacAddr,
                                            dst_mac_addr: MacAddr,
                                            ip6_datagram: &[u8]) {
    radio.config_set_pan(0xABCD);
    match src_mac_addr {
        MacAddr::ShortAddr(addr) => radio.config_set_address(addr),
        MacAddr::LongAddr(addr) => radio.config_set_address_long(addr)
    };

    let src_long = match src_mac_addr {
        MacAddr::ShortAddr(_) => false,
        MacAddr::LongAddr(_) => true
    };
    let dst_long = match dst_mac_addr {
        MacAddr::ShortAddr(_) => false,
        MacAddr::LongAddr(_) => true
    };
    let offset = radio.payload_offset(src_long, dst_long) as usize;

    let store = DummyStore {
        context0: Context {
            prefix: mesh_local_prefix,
            prefix_len: 64,
            id: 0,
            compress: true,
        }
    };
    let lowpan = LoWPAN::new(&store);
    let (consumed, written) = lowpan
        .compress(&ip6_datagram,
                  src_mac_addr,
                  dst_mac_addr,
                  &mut RF233_BUF[offset..])
        .expect("Error");
    let payload_len = ip6_datagram.len() - consumed;
    RF233_BUF[offset + written..offset + written + payload_len]
        .copy_from_slice(&ip6_datagram[consumed..ip6_datagram.len()]);

    // Transmit len is 802.15.4 header + LoWPAN-compressed packet size
    let transmit_len = radio.header_size(src_long, dst_long)
        + (written + payload_len) as u8;
    match dst_mac_addr {
        MacAddr::ShortAddr(addr) => radio.transmit(addr,
                                                   &mut RF233_BUF,
                                                   transmit_len,
                                                   src_long),
        MacAddr::LongAddr(addr) => radio.transmit_long(addr,
                                                       &mut RF233_BUF,
                                                       transmit_len,
                                                       src_long)
    };
}
