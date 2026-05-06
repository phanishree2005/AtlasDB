//! # Slotted Page System
//!
//! Core page layout (PostgreSQL-inspired):
//! ```text
//! ┌──────────────────────────────────────┐ offset 0
//! │           Page Header (24B)          │
//! ├──────────────────────────────────────┤
//! │  Slot 0 │ Slot 1 │ ... │ Slot N     │  ← grows downward
//! ├──────────────────────────────────────┤
//! │           Free Space                 │
//! ├──────────────────────────────────────┤
//! │  Tuple N │ ... │ Tuple 1 │ Tuple 0  │  ← grows upward
//! └──────────────────────────────────────┘ offset PAGE_SIZE
//! ```

use super::error::{StorageError, StorageResult};

/// Default page size (4KB). PostgreSQL uses 8KB.
pub const PAGE_SIZE: usize = 4096;
/// Page header size in bytes.
pub const PAGE_HEADER_SIZE: usize = 24;
/// Each slot entry: 2B offset + 2B length = 4 bytes.
pub const SLOT_SIZE: usize = 4;
/// Marker for a deleted/empty slot.
const SLOT_EMPTY: u16 = 0;

pub type PageId = u32;
pub type SlotId = u16;

/// Identifies a tuple globally: (page_id, slot_id).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TupleId {
    pub page_id: PageId,
    pub slot_id: SlotId,
}

/// Page header stored at offset 0.
#[derive(Debug, Clone, Copy)]
pub struct PageHeader {
    pub page_id: PageId,
    pub slot_count: u16,
    pub free_space_start: u16,
    pub free_space_end: u16,
    pub checksum: u32,
    pub flags: u16,
    pub lsn: u32,
    pub _reserved: u16,
}

impl PageHeader {
    fn serialize(&self, buf: &mut [u8]) {
        buf[0..4].copy_from_slice(&self.page_id.to_le_bytes());
        buf[4..6].copy_from_slice(&self.slot_count.to_le_bytes());
        buf[6..8].copy_from_slice(&self.free_space_start.to_le_bytes());
        buf[8..10].copy_from_slice(&self.free_space_end.to_le_bytes());
        buf[10..14].copy_from_slice(&self.checksum.to_le_bytes());
        buf[14..16].copy_from_slice(&self.flags.to_le_bytes());
        buf[16..20].copy_from_slice(&self.lsn.to_le_bytes());
        buf[20..22].copy_from_slice(&self._reserved.to_le_bytes());
        buf[22..24].copy_from_slice(&[0u8; 2]);
    }

    fn deserialize(buf: &[u8]) -> Self {
        Self {
            page_id: u32::from_le_bytes(buf[0..4].try_into().unwrap()),
            slot_count: u16::from_le_bytes(buf[4..6].try_into().unwrap()),
            free_space_start: u16::from_le_bytes(buf[6..8].try_into().unwrap()),
            free_space_end: u16::from_le_bytes(buf[8..10].try_into().unwrap()),
            checksum: u32::from_le_bytes(buf[10..14].try_into().unwrap()),
            flags: u16::from_le_bytes(buf[14..16].try_into().unwrap()),
            lsn: u32::from_le_bytes(buf[16..20].try_into().unwrap()),
            _reserved: u16::from_le_bytes(buf[20..22].try_into().unwrap()),
        }
    }
}

/// A slotted page — the fundamental unit of storage.
pub struct Page {
    pub data: [u8; PAGE_SIZE],
}

impl Page {
    /// Create a new empty page.
    pub fn new(page_id: PageId) -> Self {
        let mut page = Self { data: [0u8; PAGE_SIZE] };
        let header = PageHeader {
            page_id,
            slot_count: 0,
            free_space_start: PAGE_HEADER_SIZE as u16,
            free_space_end: PAGE_SIZE as u16,
            checksum: 0,
            flags: 0,
            lsn: 0,
            _reserved: 0,
        };
        header.serialize(&mut page.data);
        page
    }

    /// Load a page from raw bytes.
    pub fn from_bytes(bytes: &[u8; PAGE_SIZE]) -> Self {
        let mut data = [0u8; PAGE_SIZE];
        data.copy_from_slice(bytes);
        Self { data }
    }

    /// Get the page header.
    pub fn header(&self) -> PageHeader {
        PageHeader::deserialize(&self.data)
    }

    fn write_header(&mut self, header: &PageHeader) {
        header.serialize(&mut self.data);
    }

    /// Available free space in bytes.
    pub fn free_space(&self) -> usize {
        let h = self.header();
        if h.free_space_end <= h.free_space_start {
            return 0;
        }
        (h.free_space_end - h.free_space_start) as usize
    }

    /// Read a slot entry: returns (offset, length). Offset=0 means deleted.
    fn read_slot(&self, slot_id: SlotId) -> (u16, u16) {
        let pos = PAGE_HEADER_SIZE + (slot_id as usize) * SLOT_SIZE;
        let offset = u16::from_le_bytes(self.data[pos..pos + 2].try_into().unwrap());
        let length = u16::from_le_bytes(self.data[pos + 2..pos + 4].try_into().unwrap());
        (offset, length)
    }

    /// Write a slot entry.
    fn write_slot(&mut self, slot_id: SlotId, offset: u16, length: u16) {
        let pos = PAGE_HEADER_SIZE + (slot_id as usize) * SLOT_SIZE;
        self.data[pos..pos + 2].copy_from_slice(&offset.to_le_bytes());
        self.data[pos + 2..pos + 4].copy_from_slice(&length.to_le_bytes());
    }

    /// Insert a tuple. Returns the slot ID assigned.
    pub fn insert(&mut self, tuple_data: &[u8]) -> StorageResult<SlotId> {
        let data_len = tuple_data.len();
        let mut h = self.header();

        // Check if we can reuse a deleted slot
        let mut reuse_slot: Option<SlotId> = None;
        for i in 0..h.slot_count {
            let (off, _) = self.read_slot(i);
            if off == SLOT_EMPTY {
                reuse_slot = Some(i);
                break;
            }
        }

        let need_new_slot = reuse_slot.is_none();
        let extra_slot_bytes = if need_new_slot { SLOT_SIZE } else { 0 };
        let total_needed = data_len + extra_slot_bytes;

        if self.free_space() < total_needed {
            return Err(StorageError::PageFull {
                needed: total_needed,
                available: self.free_space(),
            });
        }

        if data_len > PAGE_SIZE - PAGE_HEADER_SIZE - SLOT_SIZE {
            return Err(StorageError::TupleTooLarge {
                size: data_len,
                max_size: PAGE_SIZE - PAGE_HEADER_SIZE - SLOT_SIZE,
            });
        }

        // Allocate space from the end (growing upward)
        let new_end = h.free_space_end - data_len as u16;
        self.data[new_end as usize..new_end as usize + data_len]
            .copy_from_slice(tuple_data);

        let slot_id = if let Some(sid) = reuse_slot {
            sid
        } else {
            let sid = h.slot_count;
            h.slot_count += 1;
            h.free_space_start += SLOT_SIZE as u16;
            sid
        };

        self.write_slot(slot_id, new_end, data_len as u16);
        h.free_space_end = new_end;
        self.write_header(&h);
        Ok(slot_id)
    }

    /// Read a tuple by slot ID.
    pub fn get(&self, slot_id: SlotId) -> StorageResult<&[u8]> {
        let h = self.header();
        if slot_id >= h.slot_count {
            return Err(StorageError::InvalidSlot(slot_id));
        }
        let (offset, length) = self.read_slot(slot_id);
        if offset == SLOT_EMPTY {
            return Err(StorageError::InvalidSlot(slot_id));
        }
        Ok(&self.data[offset as usize..(offset + length) as usize])
    }

    /// Delete a tuple by slot ID. Marks the slot as empty.
    pub fn delete(&mut self, slot_id: SlotId) -> StorageResult<()> {
        let h = self.header();
        if slot_id >= h.slot_count {
            return Err(StorageError::InvalidSlot(slot_id));
        }
        let (offset, _) = self.read_slot(slot_id);
        if offset == SLOT_EMPTY {
            return Err(StorageError::InvalidSlot(slot_id));
        }
        // Mark slot as empty (offset=0). Data stays until compaction.
        self.write_slot(slot_id, SLOT_EMPTY, 0);
        Ok(())
    }

    /// Update a tuple in-place if new data fits, otherwise delete + reinsert.
    pub fn update(&mut self, slot_id: SlotId, new_data: &[u8]) -> StorageResult<SlotId> {
        let h = self.header();
        if slot_id >= h.slot_count {
            return Err(StorageError::InvalidSlot(slot_id));
        }
        let (offset, length) = self.read_slot(slot_id);
        if offset == SLOT_EMPTY {
            return Err(StorageError::InvalidSlot(slot_id));
        }

        if new_data.len() <= length as usize {
            // In-place update: new data fits in existing space
            self.data[offset as usize..offset as usize + new_data.len()]
                .copy_from_slice(new_data);
            self.write_slot(slot_id, offset, new_data.len() as u16);
            Ok(slot_id)
        } else {
            // Delete old, insert new
            self.delete(slot_id)?;
            // Try to insert; if no space, compact first then retry
            match self.insert(new_data) {
                Ok(new_sid) => Ok(new_sid),
                Err(StorageError::PageFull { .. }) => {
                    self.compact();
                    self.insert(new_data)
                }
                Err(e) => Err(e),
            }
        }
    }

    /// Compact the page: eliminate dead space from deleted tuples.
    ///
    /// This is equivalent to PostgreSQL's page-level VACUUM. We rebuild
    /// the data region by copying only live tuples contiguously from the
    /// bottom of the page, then updating all slot pointers.
    pub fn compact(&mut self) {
        let h = self.header();

        // Collect live tuples: (slot_id, data)
        let mut live: Vec<(SlotId, Vec<u8>)> = Vec::new();
        for i in 0..h.slot_count {
            let (offset, length) = self.read_slot(i);
            if offset != SLOT_EMPTY && length > 0 {
                let data = self.data[offset as usize..(offset + length) as usize].to_vec();
                live.push((i, data));
            }
        }

        // Clear the data region
        let data_region_start = PAGE_HEADER_SIZE + (h.slot_count as usize) * SLOT_SIZE;
        for b in &mut self.data[data_region_start..PAGE_SIZE] {
            *b = 0;
        }

        // Rewrite tuples from the bottom, updating slot pointers
        let mut write_pos = PAGE_SIZE;
        for (sid, data) in &live {
            write_pos -= data.len();
            self.data[write_pos..write_pos + data.len()].copy_from_slice(data);
            self.write_slot(*sid, write_pos as u16, data.len() as u16);
        }

        // Update header
        let mut new_h = h;
        new_h.free_space_end = write_pos as u16;
        self.write_header(&new_h);
    }

    /// Iterate over all live tuples: yields (slot_id, data).
    pub fn iter(&self) -> PageIterator<'_> {
        PageIterator { page: self, current_slot: 0 }
    }

    /// Number of live (non-deleted) tuples.
    pub fn live_tuple_count(&self) -> usize {
        let h = self.header();
        let mut count = 0;
        for i in 0..h.slot_count {
            let (offset, _) = self.read_slot(i);
            if offset != SLOT_EMPTY {
                count += 1;
            }
        }
        count
    }

    /// Compute and store a CRC32 checksum for crash safety.
    pub fn compute_checksum(&mut self) {
        // Zero out checksum field before computing
        self.data[10..14].copy_from_slice(&0u32.to_le_bytes());
        let crc = crc32fast::hash(&self.data);
        self.data[10..14].copy_from_slice(&crc.to_le_bytes());
    }

    /// Verify the page checksum.
    pub fn verify_checksum(&self) -> bool {
        let stored = u32::from_le_bytes(self.data[10..14].try_into().unwrap());
        let mut temp = self.data;
        temp[10..14].copy_from_slice(&0u32.to_le_bytes());
        let computed = crc32fast::hash(&temp);
        stored == computed
    }
}

impl Clone for Page {
    fn clone(&self) -> Self {
        Self { data: self.data }
    }
}

impl std::fmt::Debug for Page {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let h = self.header();
        f.debug_struct("Page")
            .field("page_id", &h.page_id)
            .field("slot_count", &h.slot_count)
            .field("free_space", &self.free_space())
            .field("live_tuples", &self.live_tuple_count())
            .finish()
    }
}

/// Iterator over live tuples in a page.
pub struct PageIterator<'a> {
    page: &'a Page,
    current_slot: u16,
}

impl<'a> Iterator for PageIterator<'a> {
    type Item = (SlotId, &'a [u8]);

    fn next(&mut self) -> Option<Self::Item> {
        let h = self.page.header();
        while self.current_slot < h.slot_count {
            let sid = self.current_slot;
            self.current_slot += 1;
            if let Ok(data) = self.page.get(sid) {
                return Some((sid, data));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_page() {
        let page = Page::new(0);
        let h = page.header();
        assert_eq!(h.page_id, 0);
        assert_eq!(h.slot_count, 0);
        assert_eq!(page.free_space(), PAGE_SIZE - PAGE_HEADER_SIZE);
    }

    #[test]
    fn test_insert_and_get() {
        let mut page = Page::new(1);
        let data = b"hello world";
        let slot = page.insert(data).unwrap();
        assert_eq!(slot, 0);
        assert_eq!(page.get(slot).unwrap(), data);
    }

    #[test]
    fn test_multiple_inserts() {
        let mut page = Page::new(1);
        let s0 = page.insert(b"first").unwrap();
        let s1 = page.insert(b"second").unwrap();
        let s2 = page.insert(b"third").unwrap();
        assert_eq!(page.get(s0).unwrap(), b"first");
        assert_eq!(page.get(s1).unwrap(), b"second");
        assert_eq!(page.get(s2).unwrap(), b"third");
        assert_eq!(page.live_tuple_count(), 3);
    }

    #[test]
    fn test_delete() {
        let mut page = Page::new(1);
        let s0 = page.insert(b"delete me").unwrap();
        page.delete(s0).unwrap();
        assert!(page.get(s0).is_err());
        assert_eq!(page.live_tuple_count(), 0);
    }

    #[test]
    fn test_slot_reuse_after_delete() {
        let mut page = Page::new(1);
        let s0 = page.insert(b"first").unwrap();
        let _s1 = page.insert(b"second").unwrap();
        page.delete(s0).unwrap();
        // Next insert should reuse slot 0
        let s2 = page.insert(b"reused").unwrap();
        assert_eq!(s2, 0);
        assert_eq!(page.get(s2).unwrap(), b"reused");
    }

    #[test]
    fn test_update_in_place() {
        let mut page = Page::new(1);
        let s0 = page.insert(b"original data here").unwrap();
        let s_new = page.update(s0, b"short").unwrap();
        assert_eq!(s_new, s0);
        assert_eq!(page.get(s_new).unwrap(), b"short");
    }

    #[test]
    fn test_update_larger() {
        let mut page = Page::new(1);
        let s0 = page.insert(b"hi").unwrap();
        let s_new = page.update(s0, b"much longer replacement data").unwrap();
        assert_eq!(page.get(s_new).unwrap(), b"much longer replacement data");
    }

    #[test]
    fn test_compaction() {
        let mut page = Page::new(1);
        let s0 = page.insert(b"aaaa").unwrap();
        let _s1 = page.insert(b"bbbb").unwrap();
        let s2 = page.insert(b"cccc").unwrap();

        let free_before = page.free_space();
        page.delete(s0).unwrap();
        page.delete(s2).unwrap();

        // Free space hasn't changed yet (dead tuples still occupy space)
        let free_after_delete = page.free_space();
        assert_eq!(free_before, free_after_delete);

        page.compact();
        let free_after_compact = page.free_space();
        // After compaction, we reclaimed 8 bytes of dead tuple data
        assert!(free_after_compact > free_after_delete);
        // The surviving tuple is still accessible
        assert_eq!(page.get(1).unwrap(), b"bbbb");
        assert_eq!(page.live_tuple_count(), 1);
    }

    #[test]
    fn test_page_full() {
        let mut page = Page::new(1);
        let big = vec![0xFFu8; PAGE_SIZE]; // way too large
        assert!(page.insert(&big).is_err());
    }

    #[test]
    fn test_fill_page_then_compact() {
        let mut page = Page::new(1);
        let mut slots = Vec::new();
        // Fill with small tuples
        for i in 0..100 {
            match page.insert(format!("tuple_{:04}", i).as_bytes()) {
                Ok(s) => slots.push(s),
                Err(_) => break,
            }
        }
        let inserted = slots.len();
        assert!(inserted > 10);

        // Delete every other tuple
        for (i, &s) in slots.iter().enumerate() {
            if i % 2 == 0 {
                page.delete(s).unwrap();
            }
        }
        let live_before = page.live_tuple_count();
        page.compact();
        let live_after = page.live_tuple_count();
        assert_eq!(live_before, live_after);
        assert!(page.free_space() > 0);
    }

    #[test]
    fn test_iterator() {
        let mut page = Page::new(1);
        page.insert(b"one").unwrap();
        page.insert(b"two").unwrap();
        page.insert(b"three").unwrap();

        let items: Vec<_> = page.iter().collect();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].1, b"one");
        assert_eq!(items[1].1, b"two");
        assert_eq!(items[2].1, b"three");
    }

    #[test]
    fn test_checksum() {
        let mut page = Page::new(1);
        page.insert(b"test data").unwrap();
        page.compute_checksum();
        assert!(page.verify_checksum());
        // Corrupt a byte
        page.data[PAGE_SIZE - 1] ^= 0xFF;
        assert!(!page.verify_checksum());
    }

    #[test]
    fn test_invalid_slot_errors() {
        let page = Page::new(1);
        assert!(page.get(0).is_err());
        assert!(page.get(999).is_err());
    }
}
