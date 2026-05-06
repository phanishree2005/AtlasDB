//! # Buffer Pool
//!
//! LRU-based buffer pool managing a fixed number of page frames.
//!
//! ```text
//! ┌──────────────────────────────────────────┐
//! │  Buffer Pool (N frames)                  │
//! │  ┌──────┐ ┌──────┐ ┌──────┐ ┌──────┐   │
//! │  │Frame0│ │Frame1│ │Frame2│ │ ...  │   │
//! │  │pin:2 │ │pin:0 │ │pin:1 │ │      │   │
//! │  │dirty │ │clean │ │dirty │ │      │   │
//! │  └──────┘ └──────┘ └──────┘ └──────┘   │
//! │                                          │
//! │  Page Table: PageId -> FrameId           │
//! │  LRU List:  [Frame1, Frame2, ...]        │
//! └──────────────────────────────────────────┘
//! ```

use std::collections::{HashMap, VecDeque};

use crate::storage::disk::DiskManager;
use crate::storage::error::{StorageError, StorageResult};
use crate::storage::page::{Page, PageId};

/// Metadata for a single buffer frame.
#[derive(Debug)]
struct FrameDescriptor {
    page_id: Option<PageId>,
    pin_count: u32,
    is_dirty: bool,
}

/// A fixed-size buffer pool with LRU eviction.
pub struct BufferPool {
    /// The actual page frames.
    frames: Vec<Page>,
    /// Metadata per frame.
    descriptors: Vec<FrameDescriptor>,
    /// Maps PageId -> frame index.
    page_table: HashMap<PageId, usize>,
    /// LRU queue of unpinned frame indices (front = least recently used).
    lru: VecDeque<usize>,
    /// Disk manager for I/O.
    disk: DiskManager,
    /// Pool capacity (number of frames).
    capacity: usize,
}

impl BufferPool {
    /// Create a new buffer pool with the given capacity (number of page frames).
    pub fn new(disk: DiskManager, capacity: usize) -> Self {
        let mut frames = Vec::with_capacity(capacity);
        let mut descriptors = Vec::with_capacity(capacity);
        let mut lru = VecDeque::with_capacity(capacity);

        for i in 0..capacity {
            frames.push(Page::new(0));
            descriptors.push(FrameDescriptor {
                page_id: None,
                pin_count: 0,
                is_dirty: false,
            });
            lru.push_back(i);
        }

        Self {
            frames,
            descriptors,
            page_table: HashMap::new(),
            lru,
            disk,
            capacity,
        }
    }

    /// Fetch a page into the buffer pool and pin it.
    /// Returns a reference to the page. The caller must call `unpin` when done.
    pub fn fetch_page(&mut self, page_id: PageId) -> StorageResult<&Page> {
        // Check if already in pool
        if let Some(&frame_id) = self.page_table.get(&page_id) {
            self.descriptors[frame_id].pin_count += 1;
            // Remove from LRU if present (it's now pinned)
            self.lru.retain(|&x| x != frame_id);
            return Ok(&self.frames[frame_id]);
        }

        // Need to load from disk — find a free frame
        let frame_id = self.find_victim()?;

        // Evict the old page if necessary
        if let Some(old_pid) = self.descriptors[frame_id].page_id {
            if self.descriptors[frame_id].is_dirty {
                self.disk.write_page(old_pid, &self.frames[frame_id])?;
            }
            self.page_table.remove(&old_pid);
        }

        // Load the new page
        let page = self.disk.read_page(page_id)?;
        self.frames[frame_id] = page;
        self.descriptors[frame_id] = FrameDescriptor {
            page_id: Some(page_id),
            pin_count: 1,
            is_dirty: false,
        };
        self.page_table.insert(page_id, frame_id);
        self.lru.retain(|&x| x != frame_id);

        Ok(&self.frames[frame_id])
    }

    /// Fetch a mutable reference to a page. Automatically marks it dirty.
    pub fn fetch_page_mut(&mut self, page_id: PageId) -> StorageResult<&mut Page> {
        // Check if already in pool
        if let Some(&frame_id) = self.page_table.get(&page_id) {
            self.descriptors[frame_id].pin_count += 1;
            self.descriptors[frame_id].is_dirty = true;
            self.lru.retain(|&x| x != frame_id);
            return Ok(&mut self.frames[frame_id]);
        }

        let frame_id = self.find_victim()?;

        if let Some(old_pid) = self.descriptors[frame_id].page_id {
            if self.descriptors[frame_id].is_dirty {
                self.disk.write_page(old_pid, &self.frames[frame_id])?;
            }
            self.page_table.remove(&old_pid);
        }

        let page = self.disk.read_page(page_id)?;
        self.frames[frame_id] = page;
        self.descriptors[frame_id] = FrameDescriptor {
            page_id: Some(page_id),
            pin_count: 1,
            is_dirty: true,
        };
        self.page_table.insert(page_id, frame_id);
        self.lru.retain(|&x| x != frame_id);

        Ok(&mut self.frames[frame_id])
    }

    /// Allocate a new page in the buffer pool.
    pub fn new_page(&mut self) -> StorageResult<(PageId, &mut Page)> {
        let page_id = self.disk.allocate_page()?;
        let frame_id = self.find_victim()?;

        if let Some(old_pid) = self.descriptors[frame_id].page_id {
            if self.descriptors[frame_id].is_dirty {
                self.disk.write_page(old_pid, &self.frames[frame_id])?;
            }
            self.page_table.remove(&old_pid);
        }

        self.frames[frame_id] = Page::new(page_id);
        self.descriptors[frame_id] = FrameDescriptor {
            page_id: Some(page_id),
            pin_count: 1,
            is_dirty: true,
        };
        self.page_table.insert(page_id, frame_id);
        self.lru.retain(|&x| x != frame_id);

        Ok((page_id, &mut self.frames[frame_id]))
    }

    /// Unpin a page. When pin_count reaches 0, the frame becomes evictable.
    pub fn unpin(&mut self, page_id: PageId, is_dirty: bool) -> StorageResult<()> {
        let frame_id = *self.page_table.get(&page_id).ok_or_else(|| {
            StorageError::PageNotFound(page_id)
        })?;

        if self.descriptors[frame_id].pin_count == 0 {
            return Err(StorageError::InvalidTuple(
                format!("page {} already unpinned", page_id)
            ));
        }

        self.descriptors[frame_id].pin_count -= 1;
        if is_dirty {
            self.descriptors[frame_id].is_dirty = true;
        }

        if self.descriptors[frame_id].pin_count == 0 {
            self.lru.push_back(frame_id);
        }
        Ok(())
    }

    /// Flush a specific page to disk.
    pub fn flush_page(&mut self, page_id: PageId) -> StorageResult<()> {
        if let Some(&frame_id) = self.page_table.get(&page_id) {
            if self.descriptors[frame_id].is_dirty {
                self.disk.write_page(page_id, &self.frames[frame_id])?;
                self.descriptors[frame_id].is_dirty = false;
            }
        }
        Ok(())
    }

    /// Flush all dirty pages to disk.
    pub fn flush_all(&mut self) -> StorageResult<()> {
        for frame_id in 0..self.capacity {
            if let Some(page_id) = self.descriptors[frame_id].page_id {
                if self.descriptors[frame_id].is_dirty {
                    self.disk.write_page(page_id, &self.frames[frame_id])?;
                    self.descriptors[frame_id].is_dirty = false;
                }
            }
        }
        self.disk.sync()?;
        Ok(())
    }

    /// Find a victim frame for eviction (LRU policy).
    fn find_victim(&mut self) -> StorageResult<usize> {
        // First try to find an empty frame
        for i in 0..self.capacity {
            if self.descriptors[i].page_id.is_none() && self.descriptors[i].pin_count == 0 {
                self.lru.retain(|&x| x != i);
                return Ok(i);
            }
        }

        // Otherwise, evict the LRU unpinned frame
        if let Some(frame_id) = self.lru.pop_front() {
            return Ok(frame_id);
        }

        Err(StorageError::InvalidTuple(
            "buffer pool is full: all frames are pinned".into()
        ))
    }

    /// Number of pages currently in the pool.
    pub fn size(&self) -> usize {
        self.page_table.len()
    }

    /// Get the underlying disk manager (for page count queries).
    pub fn disk(&self) -> &DiskManager {
        &self.disk
    }

    /// Get mutable access to the disk manager.
    pub fn disk_mut(&mut self) -> &mut DiskManager {
        &mut self.disk
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn setup(capacity: usize) -> (BufferPool, tempfile::NamedTempFile) {
        let tmp = NamedTempFile::new().unwrap();
        let dm = DiskManager::open(tmp.path()).unwrap();
        let pool = BufferPool::new(dm, capacity);
        (pool, tmp)
    }

    #[test]
    fn test_new_page_and_fetch() {
        let (mut pool, _tmp) = setup(4);
        let (pid, page) = pool.new_page().unwrap();
        page.insert(b"hello buffer").unwrap();
        pool.unpin(pid, true).unwrap();

        let page = pool.fetch_page(pid).unwrap();
        assert_eq!(page.get(0).unwrap(), b"hello buffer");
        pool.unpin(pid, false).unwrap();
    }

    #[test]
    fn test_dirty_flush() {
        let (mut pool, _tmp) = setup(4);
        let (pid, page) = pool.new_page().unwrap();
        page.insert(b"persistent data").unwrap();
        pool.unpin(pid, true).unwrap();
        pool.flush_all().unwrap();

        // Re-fetch should still have the data
        let page = pool.fetch_page(pid).unwrap();
        assert_eq!(page.get(0).unwrap(), b"persistent data");
        pool.unpin(pid, false).unwrap();
    }

    #[test]
    fn test_lru_eviction() {
        // Pool size = 2, allocate 3 pages → oldest gets evicted
        let (mut pool, _tmp) = setup(2);

        let (p0, page) = pool.new_page().unwrap();
        page.insert(b"page0").unwrap();
        pool.unpin(p0, true).unwrap();

        let (p1, page) = pool.new_page().unwrap();
        page.insert(b"page1").unwrap();
        pool.unpin(p1, true).unwrap();

        // Allocate a 3rd → should evict p0 (LRU)
        let (p2, page) = pool.new_page().unwrap();
        page.insert(b"page2").unwrap();
        pool.unpin(p2, true).unwrap();

        // p0 was evicted but was dirty, so data should be on disk
        // Fetching p0 loads it back (evicting p1)
        let page = pool.fetch_page(p0).unwrap();
        assert_eq!(page.get(0).unwrap(), b"page0");
        pool.unpin(p0, false).unwrap();
    }

    #[test]
    fn test_pin_prevents_eviction() {
        let (mut pool, _tmp) = setup(2);

        let (p0, _) = pool.new_page().unwrap();
        // p0 is pinned (pin_count = 1)

        let (p1, _) = pool.new_page().unwrap();
        pool.unpin(p1, true).unwrap();
        // p1 is unpinned

        // Allocate p2 — should evict p1 (unpinned), not p0 (pinned)
        let (p2, _) = pool.new_page().unwrap();
        pool.unpin(p2, true).unwrap();

        // p0 should still be in pool
        assert!(pool.page_table.contains_key(&p0));
        pool.unpin(p0, false).unwrap();
    }

    #[test]
    fn test_unpin_double_error() {
        let (mut pool, _tmp) = setup(4);
        let (pid, _) = pool.new_page().unwrap();
        pool.unpin(pid, false).unwrap();
        // Second unpin should error
        assert!(pool.unpin(pid, false).is_err());
    }
}
