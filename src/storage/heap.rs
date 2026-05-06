//! # Heap File
//!
//! A heap file is an unordered collection of pages storing table data.
//! Tuples are inserted into the first page with sufficient free space.
//! This mirrors PostgreSQL's heap access method.

use super::disk::DiskManager;
use super::error::{StorageError, StorageResult};
use super::page::{Page, TupleId};
use super::tuple::Tuple;

/// Manages a heap file (one per table).
pub struct HeapFile {
    disk: DiskManager,
    table_name: String,
}

impl HeapFile {
    /// Open or create a heap file for a table.
    pub fn open(path: &str, table_name: &str) -> StorageResult<Self> {
        let disk = DiskManager::open(path)?;
        Ok(Self {
            disk,
            table_name: table_name.to_string(),
        })
    }

    /// Insert a tuple. Returns its TupleId (page_id, slot_id).
    pub fn insert_tuple(&mut self, tuple: &Tuple) -> StorageResult<TupleId> {
        let size = tuple.serialized_size();
        let mut buf = vec![0u8; size];
        tuple.serialize(&mut buf)?;

        // Try existing pages
        for pid in 0..self.disk.num_pages() {
            let mut page = self.disk.read_page(pid)?;
            if page.free_space() >= size + 4 {
                let slot_id = page.insert(&buf)?;
                self.disk.write_page(pid, &page)?;
                return Ok(TupleId { page_id: pid, slot_id });
            }
        }

        // Allocate a new page
        let pid = self.disk.allocate_page()?;
        let mut page = Page::new(pid);
        let slot_id = page.insert(&buf)?;
        self.disk.write_page(pid, &page)?;
        Ok(TupleId { page_id: pid, slot_id })
    }

    /// Read a tuple by TupleId.
    pub fn get_tuple(&mut self, tid: TupleId) -> StorageResult<Tuple> {
        let page = self.disk.read_page(tid.page_id)?;
        let data = page.get(tid.slot_id)?;
        Tuple::deserialize(data)
    }

    /// Delete a tuple by TupleId.
    pub fn delete_tuple(&mut self, tid: TupleId) -> StorageResult<()> {
        let mut page = self.disk.read_page(tid.page_id)?;
        page.delete(tid.slot_id)?;
        self.disk.write_page(tid.page_id, &page)?;
        Ok(())
    }

    /// Update a tuple. Returns the (possibly new) TupleId.
    pub fn update_tuple(&mut self, tid: TupleId, new_tuple: &Tuple) -> StorageResult<TupleId> {
        let size = new_tuple.serialized_size();
        let mut buf = vec![0u8; size];
        new_tuple.serialize(&mut buf)?;

        let mut page = self.disk.read_page(tid.page_id)?;
        match page.update(tid.slot_id, &buf) {
            Ok(new_slot) => {
                self.disk.write_page(tid.page_id, &page)?;
                Ok(TupleId { page_id: tid.page_id, slot_id: new_slot })
            }
            Err(StorageError::PageFull { .. }) => {
                // Delete from old page, insert into a page with space
                page.delete(tid.slot_id)?;
                self.disk.write_page(tid.page_id, &page)?;
                self.insert_tuple(new_tuple)
            }
            Err(e) => Err(e),
        }
    }

    /// Sequential scan: iterate all live tuples across all pages.
    pub fn scan(&mut self) -> StorageResult<Vec<(TupleId, Tuple)>> {
        let mut results = Vec::new();
        for pid in 0..self.disk.num_pages() {
            let page = self.disk.read_page(pid)?;
            for (slot_id, data) in page.iter() {
                match Tuple::deserialize(data) {
                    Ok(tuple) if tuple.header.is_live() => {
                        results.push((TupleId { page_id: pid, slot_id }, tuple));
                    }
                    _ => {}
                }
            }
        }
        Ok(results)
    }

    /// Flush to disk.
    pub fn sync(&mut self) -> StorageResult<()> {
        self.disk.sync()
    }

    /// Table name.
    pub fn table_name(&self) -> &str {
        &self.table_name
    }

    /// Total number of pages.
    pub fn num_pages(&self) -> u32 {
        self.disk.num_pages()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::tuple::FieldValue;
    use tempfile::NamedTempFile;

    fn make_tuple(id: i32, name: &str) -> Tuple {
        Tuple::new(1, vec![
            FieldValue::Int32(id),
            FieldValue::String(name.to_string()),
        ])
    }

    #[test]
    fn test_heap_insert_and_get() {
        let tmp = NamedTempFile::new().unwrap();
        let mut heap = HeapFile::open(
            tmp.path().to_str().unwrap(), "users"
        ).unwrap();

        let t = make_tuple(1, "Alice");
        let tid = heap.insert_tuple(&t).unwrap();
        let restored = heap.get_tuple(tid).unwrap();
        assert_eq!(restored.fields, t.fields);
    }

    #[test]
    fn test_heap_delete() {
        let tmp = NamedTempFile::new().unwrap();
        let mut heap = HeapFile::open(
            tmp.path().to_str().unwrap(), "users"
        ).unwrap();

        let tid = heap.insert_tuple(&make_tuple(1, "Alice")).unwrap();
        heap.delete_tuple(tid).unwrap();
        assert!(heap.get_tuple(tid).is_err());
    }

    #[test]
    fn test_heap_scan() {
        let tmp = NamedTempFile::new().unwrap();
        let mut heap = HeapFile::open(
            tmp.path().to_str().unwrap(), "users"
        ).unwrap();

        for i in 0..10 {
            heap.insert_tuple(&make_tuple(i, &format!("user_{}", i))).unwrap();
        }

        let results = heap.scan().unwrap();
        assert_eq!(results.len(), 10);
    }

    #[test]
    fn test_heap_update() {
        let tmp = NamedTempFile::new().unwrap();
        let mut heap = HeapFile::open(
            tmp.path().to_str().unwrap(), "users"
        ).unwrap();

        let tid = heap.insert_tuple(&make_tuple(1, "Alice")).unwrap();
        let new_tid = heap.update_tuple(tid, &make_tuple(1, "Alice Updated")).unwrap();
        let restored = heap.get_tuple(new_tid).unwrap();
        assert_eq!(restored.fields[1], FieldValue::String("Alice Updated".into()));
    }

    #[test]
    fn test_heap_multi_page() {
        let tmp = NamedTempFile::new().unwrap();
        let mut heap = HeapFile::open(
            tmp.path().to_str().unwrap(), "big_table"
        ).unwrap();

        // Insert enough tuples to span multiple pages
        for i in 0..200 {
            let long_name = format!("user_with_a_long_name_{:04}", i);
            heap.insert_tuple(&make_tuple(i, &long_name)).unwrap();
        }
        heap.sync().unwrap();

        assert!(heap.num_pages() > 1);
        let results = heap.scan().unwrap();
        assert_eq!(results.len(), 200);
    }
}
