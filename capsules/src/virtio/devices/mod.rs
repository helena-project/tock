//! VirtIO device drivers in Tock
//!
//! Currently, the following types of VirtIO devices are supported:
//!
//! - [Entropy Source](virtio_rng::VirtIORng)
//! - [Network Card](virtio_net::VirtIONet)

pub mod virtio_net;
pub mod virtio_rng;
