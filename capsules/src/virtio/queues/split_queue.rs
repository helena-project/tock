use core::cell::Cell;
use core::cmp;
use kernel::common::cells::OptionalCell;
use kernel::common::registers::{register_bitfields, InMemoryRegister};
use kernel::ErrorCode;

use crate::virtio::{VirtIOTransport, VirtQueue, VirtQueueAddresses};

pub const DESCRIPTOR_ALIGNMENT: usize = 16;
pub const AVAILABLE_RING_ALIGNMENT: usize = 2;
pub const USED_RING_ALIGNMENT: usize = 4;

const QUEUE_MAX_ELEMENTS: usize = 2;

#[repr(C)]
pub struct VirtQueueDescriptor {
    /// Guest physical address of the buffer to share
    addr: InMemoryRegister<u64>,
    /// Length of the shared buffer
    len: InMemoryRegister<u32>,
    /// Descriptor flags
    flags: InMemoryRegister<u16, DescriptorFlags::Register>,
    /// Pointer to the next entry in the descriptor queue (if two
    /// buffers are chained)
    next: InMemoryRegister<u16>,
}

impl Default for VirtQueueDescriptor {
    fn default() -> VirtQueueDescriptor {
        VirtQueueDescriptor {
            addr: InMemoryRegister::new(0),
            len: InMemoryRegister::new(0),
            flags: InMemoryRegister::new(0),
            next: InMemoryRegister::new(0),
        }
    }
}

#[derive(Default)]
#[repr(C, align(16))]
pub struct VirtQueueDescriptors([VirtQueueDescriptor; QUEUE_MAX_ELEMENTS]);

#[repr(C, align(2))]
pub struct VirtQueueAvailableRing {
    /// Available ring flags
    flags: InMemoryRegister<u16, AvailableRingFlags::Register>,
    /// Incrementing index (where the driver would put the next
    /// descriptor entry in the ring, modulo the queue size)
    idx: InMemoryRegister<u16>,
    /// Ring containing the shared buffers (descriptor indicies modulo
    /// the descriptor size)
    ring: [VirtQueueAvailableElement; QUEUE_MAX_ELEMENTS],
    /// Used event (only if EventIdx is negotiated)
    ///
    /// Can be used for event suppression
    used_event: InMemoryRegister<u16>,
}

// This is required to be able to implement Default and hence to
// initialize an entire array of default values with size specified by
// a constant
#[repr(C)]
pub struct VirtQueueAvailableElement(InMemoryRegister<u16>);

impl Default for VirtQueueAvailableElement {
    fn default() -> VirtQueueAvailableElement {
        VirtQueueAvailableElement(InMemoryRegister::new(0))
    }
}

impl Default for VirtQueueAvailableRing {
    fn default() -> VirtQueueAvailableRing {
        VirtQueueAvailableRing {
            flags: InMemoryRegister::new(0),
            idx: InMemoryRegister::new(0),
            ring: Default::default(),
            used_event: InMemoryRegister::new(0),
        }
    }
}

#[repr(C, align(4))]
pub struct VirtQueueUsedRing {
    /// Used ring flags
    flags: InMemoryRegister<u16, UsedRingFlags::Register>,
    /// Incrementing index (where the device would put the next
    /// descriptor entry in the ring, modulo the queue size)
    idx: InMemoryRegister<u16>,
    /// Ring containing the used buffers (descriptor indices modulo
    /// the descriptor size)
    ring: [VirtQueueUsedElement; QUEUE_MAX_ELEMENTS],
    /// Available event (only if EventIdx is negotiated)
    ///
    /// Can be used for event suppression
    avail_event: InMemoryRegister<u16>,
}

impl Default for VirtQueueUsedRing {
    fn default() -> VirtQueueUsedRing {
        VirtQueueUsedRing {
            flags: InMemoryRegister::new(0),
            idx: InMemoryRegister::new(0),
            ring: Default::default(),
            avail_event: InMemoryRegister::new(0),
        }
    }
}

#[repr(C)]
pub struct VirtQueueUsedElement {
    /// Index (offset modulo descriptor size) of the start of the used
    /// descriptor chain
    id: InMemoryRegister<u32>,
    /// Total length of the descriptor chain which was used (written
    /// to)
    len: InMemoryRegister<u32>,
}

impl Default for VirtQueueUsedElement {
    fn default() -> VirtQueueUsedElement {
        VirtQueueUsedElement {
            id: InMemoryRegister::new(0),
            len: InMemoryRegister::new(0),
        }
    }
}

register_bitfields![u16,
            DescriptorFlags [
            Next OFFSET(0) NUMBITS(1) [],
            WriteOnly OFFSET(1) NUMBITS(1) [],
            Indirect OFFSET(2) NUMBITS(1) []
            ],
            AvailableRingFlags [
            NoInterrupt OFFSET(0) NUMBITS(1) []
            ],
            UsedRingFlags [
            NoNotify OFFSET(0) NUMBITS(1) []
            ]
];

pub struct AvailableRingHandler {
    max_elements: Cell<usize>,
    start: Cell<u16>,
    end: Cell<u16>,
    empty: Cell<bool>,
}

impl AvailableRingHandler {
    pub fn new(max_elements: usize) -> AvailableRingHandler {
        AvailableRingHandler {
            max_elements: Cell::new(max_elements),
            start: Cell::new(0),
            end: Cell::new(0),
            empty: Cell::new(true),
        }
    }

    fn ring_wrapping_add(&self, a: u16, b: u16) -> u16 {
        if self.max_elements.get() - (a as usize) - 1 < (b as usize) {
            b - (self.max_elements.get() - a as usize) as u16
        } else {
            a + b
        }
    }

    pub fn reset(&self, max_elements: usize) {
        self.max_elements.set(max_elements);
        self.start.set(0);
        self.end.set(0);
        self.empty.set(true);
    }

    pub fn is_empty(&self) -> bool {
        self.empty.get()
    }

    pub fn is_full(&self) -> bool {
        !self.empty.get() && self.start.get() == self.end.get()
    }

    pub fn insert(&self) -> Option<u16> {
        if !self.is_full() {
            let pos = self.end.get();
            self.end.set(self.ring_wrapping_add(pos, 1));
            self.empty.set(false);
            Some(pos)
        } else {
            None
        }
    }

    pub fn pop(&self) -> Option<u16> {
        if !self.is_empty() {
            let pos = self.start.get();
            self.start.set(self.ring_wrapping_add(pos, 1));
            if self.start.get() == self.end.get() {
                self.empty.set(true);
            }
            Some(pos)
        } else {
            None
        }
    }

    pub fn next_index(&self) -> u16 {
        self.start.get()
    }
}

pub struct SplitVirtQueue<'a, 'b> {
    descriptors: &'a mut VirtQueueDescriptors,
    available_ring: &'a mut VirtQueueAvailableRing,
    used_ring: &'a mut VirtQueueUsedRing,

    available_ring_state: AvailableRingHandler,
    last_used_idx: Cell<u16>,

    transport: OptionalCell<&'a dyn VirtIOTransport>,

    initialized: Cell<bool>,
    queue_number: Cell<u32>,
    max_elements: Cell<usize>,

    descriptor_buffers: [OptionalCell<&'b mut [u8]>; QUEUE_MAX_ELEMENTS],

    client: OptionalCell<&'a dyn SplitVirtQueueClient<'b>>,
    used_callbacks_enabled: Cell<bool>,
}

pub struct VirtQueueBuffer<'b> {
    pub buf: &'b mut [u8],
    pub len: usize,
    pub device_writable: bool,
}

impl<'a, 'b> SplitVirtQueue<'a, 'b> {
    pub fn new(
        descriptors: &'a mut VirtQueueDescriptors,
        available_ring: &'a mut VirtQueueAvailableRing,
        used_ring: &'a mut VirtQueueUsedRing,
    ) -> SplitVirtQueue<'a, 'b> {
        assert!(descriptors as *const _ as usize % DESCRIPTOR_ALIGNMENT == 0);
        assert!(available_ring as *const _ as usize % AVAILABLE_RING_ALIGNMENT == 0);
        assert!(used_ring as *const _ as usize % USED_RING_ALIGNMENT == 0);

        SplitVirtQueue {
            descriptors,
            available_ring,
            used_ring,

            available_ring_state: AvailableRingHandler::new(QUEUE_MAX_ELEMENTS),
            last_used_idx: Cell::new(0),

            transport: OptionalCell::empty(),

            initialized: Cell::new(false),
            queue_number: Cell::new(0),
            max_elements: Cell::new(QUEUE_MAX_ELEMENTS),

            descriptor_buffers: Default::default(),

            client: OptionalCell::empty(),
            used_callbacks_enabled: Cell::new(false),
        }
    }

    pub fn set_client(&self, client: &'a dyn SplitVirtQueueClient<'b>) {
        self.client.set(client);
    }

    pub fn set_transport(&self, transport: &'a dyn VirtIOTransport) {
        self.transport.set(transport);
    }

    pub fn queue_number(&self) -> u32 {
        assert!(self.initialized.get());
        self.queue_number.get()
    }

    /// Return the number of free descriptor slots in the descriptor
    /// table
    ///
    /// This takes into account the negotiated max queue length
    pub fn free_descriptor_count(&self) -> usize {
        assert!(self.initialized.get());
        self.descriptor_buffers
            .iter()
            .take(self.max_elements.get())
            .fold(0, |count, descbuf_entry| {
                if descbuf_entry.is_none() {
                    count + 1
                } else {
                    count
                }
            })
    }

    pub fn used_descriptor_chains_count(&self) -> usize {
        let pending_chains = self
            .used_ring
            .idx
            .get()
            .wrapping_sub(self.last_used_idx.get());

        // If we ever have more than max_elements pending descriptors,
        // the used ring increased too fast and has overwritten data
        assert!(pending_chains as usize <= self.max_elements.get());

        pending_chains as usize
    }

    /// Remove an element from the device's used queue
    ///
    /// If `self.last_used_idx.get() == self.used_ring.idx.get()`
    /// (e.g. no new used buffer chain) this will return None
    ///
    /// This will update `self.last_used_idx`.
    ///
    /// The caller must appropriately manage the available ring,
    /// freeing one entry if a used buffer was returned to.
    fn remove_used_chain(&self) -> Option<(usize, usize)> {
        assert!(self.initialized.get());

        let pending_chains = self.used_descriptor_chains_count();

        if pending_chains > 0 {
            let last_used_idx = self.last_used_idx.get();

            // Remove the element one below the index (as 0 indicates
            // _no_ buffer has been written), hence the index points
            // to the next element to be written
            let ring_pos = (last_used_idx as usize) % self.max_elements.get();
            let chain_top_idx = self.used_ring.ring[ring_pos].id.get();
            let written_len = self.used_ring.ring[ring_pos].len.get();

            // Increment our local idx counter
            self.last_used_idx.set(last_used_idx.wrapping_add(1));

            Some((chain_top_idx as usize, written_len as usize))
        } else {
            None
        }
    }

    /// Add an element to the available queue, returning either the
    /// index or None if there is no sufficient space
    ///
    /// It is the callers job to notify the device about the new
    /// available buffers
    fn add_available_descriptor(&self, descriptor_chain_head: usize) -> Option<usize> {
        assert!(self.initialized.get());

        if let Some(element_pos) = self.available_ring_state.insert() {
            // Write the element
            self.available_ring.ring[element_pos as usize]
                .0
                .set(descriptor_chain_head as u16);

            // Update the idx
            self.available_ring
                .idx
                .set(self.available_ring.idx.get().wrapping_add(1));

            Some(element_pos as usize)
        } else {
            None
        }
    }

    fn add_descriptor_chain(
        &self,
        buffer_chain: &mut [Option<VirtQueueBuffer<'b>>],
    ) -> Result<usize, ErrorCode> {
        assert!(self.initialized.get());

        // Get size of actual chain, until the first None
        let queue_length = buffer_chain
            .iter()
            .take_while(|elem| elem.is_some())
            .count();

        // Make sure we have sufficient space available
        //
        // This takes into account the negotiated max size and will
        // only list free iterators within that range
        if self.free_descriptor_count() < queue_length {
            return Err(ErrorCode::NOMEM);
        }

        // Walk over the descriptor table & buffer chain in parallel,
        // inserting where empty
        //
        // We don't need to do any bounds checking here, if we run
        // over the boundary it's safe to panic as something is
        // seriously wrong with `free_descriptor_count`
        let mut i = 0;
        let mut previous_descriptor: Option<usize> = None;
        let mut head = None;
        let queuebuf_iter = buffer_chain.iter_mut().peekable();
        for queuebuf in queuebuf_iter.take_while(|queuebuf| queuebuf.is_some()) {
            // Take the queuebuf out of the caller array
            let taken_queuebuf = queuebuf.take().expect("queuebuf is None");

            // Sanity check the buffer: the subslice length may never
            // exceed the slice length
            assert!(taken_queuebuf.buf.len() >= taken_queuebuf.len);

            while self.descriptor_buffers[i].is_some() {
                i += 1;

                // We should never run over the end, as we should have
                // sufficient free descriptors
                assert!(i < self.descriptor_buffers.len());
            }

            // Alright, we found a slot to insert the descriptor
            //
            // Check if it's the first one and store it's index as head
            if head.is_none() {
                head = Some(i);
            }

            // Write out the descriptor
            let desc = &self.descriptors.0[i];
            desc.len.set(taken_queuebuf.len as u32);
            assert!(desc.len.get() > 0);
            desc.addr.set(taken_queuebuf.buf.as_ptr() as u64);
            desc.flags.write(if taken_queuebuf.device_writable {
                DescriptorFlags::WriteOnly::SET
            } else {
                DescriptorFlags::WriteOnly::CLEAR
            });

            // Now that we know our descriptor position, check whether
            // we must chain ourself to a previous descriptor
            if let Some(prev_index) = previous_descriptor {
                self.descriptors.0[prev_index]
                    .flags
                    .modify(DescriptorFlags::Next::SET);
                self.descriptors.0[prev_index].next.set(i as u16);
            }

            // Finally, store the full slice for reference
            self.descriptor_buffers[i].replace(taken_queuebuf.buf);

            // Set ourself as the previous descriptor, as we know the
            // position of `next` only in the next loop iteration.
            previous_descriptor = Some(i);

            // Increment the counter to not check the current
            // descriptor entry again
            i += 1;
        }

        Ok(head.expect("No head added to the descriptor table"))
    }

    fn remove_descriptor_chain(
        &self,
        top_descriptor_index: usize,
    ) -> [Option<VirtQueueBuffer<'b>>; QUEUE_MAX_ELEMENTS] {
        assert!(self.initialized.get());

        let mut res: [Option<VirtQueueBuffer<'b>>; QUEUE_MAX_ELEMENTS] = Default::default();
        let mut i = 0;
        let mut next_index: Option<usize> = Some(top_descriptor_index);

        while let Some(current_index) = next_index.clone() {
            // Get a reference over the current descriptor
            let current_desc = &self.descriptors.0[current_index];

            // Check whether we have a chained descriptor and store that in next_index
            if current_desc.flags.is_set(DescriptorFlags::Next) {
                next_index = Some(current_desc.next.get() as usize);
            } else {
                next_index = None;
            }

            // Recover the slice originally associated with this
            // descriptor & delete it from the buffers array
            //
            // The caller may have provided us a larger Rust slice,
            // but indicated to only provide a subslice to VirtIO,
            // hence we'll use the stored original slice and also
            // return the subslice length
            let supplied_slice = self.descriptor_buffers[current_index]
                .take()
                .expect("VirtQueue descriptors and slices out of sync");
            assert!(supplied_slice.as_ptr() as u64 == current_desc.addr.get());

            // Reconstruct the input VirtQueueBuffer to hand it back
            res[i] = Some(VirtQueueBuffer {
                buf: supplied_slice,
                len: current_desc.len.get() as usize,
                device_writable: current_desc.flags.is_set(DescriptorFlags::WriteOnly),
            });

            // Zero the descriptor
            current_desc.addr.set(0);
            current_desc.len.set(0);
            current_desc.flags.set(0);
            current_desc.next.set(0);

            // Increment the loop iterator
            i += 1;
        }

        res
    }

    pub fn provide_buffer_chain(
        &self,
        buffer_chain: &mut [Option<VirtQueueBuffer<'b>>],
    ) -> Result<(), ErrorCode> {
        assert!(self.initialized.get());

        // Try to add the chain into the descriptor array
        //
        // If there is sufficient space available, there should also
        // be sufficient space in the available ring
        let descriptor_chain_head = self.add_descriptor_chain(buffer_chain)?;

        // Now make it available to the device
        self.add_available_descriptor(descriptor_chain_head)
            .expect("Insufficient space in available ring");

        // Notify the queue
        self.transport
            .map(|t| t.queue_notify(self.queue_number.get()))
            .expect("Missing transport reference");

        Ok(())
    }

    pub fn pop_used_descriptor_chain(
        &self,
    ) -> Option<([Option<VirtQueueBuffer<'b>>; QUEUE_MAX_ELEMENTS], usize)> {
        assert!(self.initialized.get());

        self.remove_used_chain()
            .map(|(descriptor_idx, bytes_used)| {
                // Get the descriptor chain
                let chain = self.remove_descriptor_chain(descriptor_idx);

                // Remove the first entry of the available ring, since we
                // got a single buffer back and can therefore make another
                // buffer available to the device without risking an
                // overflow of the used ring
                self.available_ring_state.pop();

                (chain, bytes_used)
            })
    }

    pub fn enable_used_callbacks(&self) {
        self.used_callbacks_enabled.set(true);
    }

    pub fn disable_used_callbacks(&self) {
        self.used_callbacks_enabled.set(false);
    }
}

impl<'a, 'b> VirtQueue for SplitVirtQueue<'a, 'b> {
    fn used_interrupt(&self) {
        assert!(self.initialized.get());
        // A buffer MAY have been put into the used in by the device
        //
        // Try to extract all pending used buffers and return them to
        // the clients via callbacks

        while self.used_callbacks_enabled.get() {
            if let Some((mut chain, bytes_used)) = self.pop_used_descriptor_chain() {
                self.client.map(move |client| {
                    client.buffer_chain_ready(self.queue_number.get(), chain.as_mut(), bytes_used)
                });
            } else {
                break;
            }
        }
    }

    fn physical_addresses(&self) -> VirtQueueAddresses {
        VirtQueueAddresses {
            descriptor_area: self.descriptors as *const _ as u64,
            driver_area: self.available_ring as *const _ as u64,
            device_area: self.used_ring as *const _ as u64,
        }
    }

    fn negotiate_queue_size(&self, max_elements: usize) -> usize {
        assert!(self.initialized.get() == false);
        let negotiated = cmp::min(QUEUE_MAX_ELEMENTS, max_elements);
        self.max_elements.set(negotiated);
        self.available_ring_state.reset(negotiated);
        negotiated
    }

    fn initialize(&self, queue_number: u32) {
        assert!(self.initialized.get() == false);

        // TODO: Zero the queue
        //
        // For now we assume all passed in queue buffers are actually
        // zeroed

        self.queue_number.set(queue_number);
        self.initialized.set(true);
    }

    fn negotiated_queue_size(&self) -> usize {
        assert!(self.initialized.get());
        self.max_elements.get()
    }
}

pub trait SplitVirtQueueClient<'b> {
    fn buffer_chain_ready(
        &self,
        queue_number: u32,
        buffer_chain: &mut [Option<VirtQueueBuffer<'b>>],
        bytes_used: usize,
    );
}
