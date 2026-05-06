//! # Disk Manager
//!
//! Handles reading/writing pages to/from disk files.
//! Each table gets its own segment file on disk.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use super::error::{StorageError, StorageResult};
use super::page::{Page, PageId, PAGE_SIZE};

/// Manages page-level I/O for a single database file.
pub struct DiskManager {
    file: File,
    file_path: String,
    num_pages: u32,
}

impl DiskManager {
    /// Open or create a database file.
    pub fn open<P: AsRef<Path>>(path: P) -> StorageResult<Self> {
        let file_path = path.as_ref().to_string_lossy().to_string();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;

        let metadata = file.metadata()?;
        let file_size = metadata.len();
        let num_pages = (file_size / PAGE_SIZE as u64) as u32;

        Ok(Self { file, file_path, num_pages })
    }

    /// Read a page from disk.
    pub fn read_page(&mut self, page_id: PageId) -> StorageResult<Page> {
        if page_id >= self.num_pages {
            return Err(StorageError::PageNotFound(page_id));
        }
        let offset = (page_id as u64) * (PAGE_SIZE as u64);
        self.file.seek(SeekFrom::Start(offset))?;
        let mut buf = [0u8; PAGE_SIZE];
        self.file.read_exact(&mut buf)?;
        Ok(Page::from_bytes(&buf))
    }

    /// Write a page to disk.
    pub fn write_page(&mut self, page_id: PageId, page: &Page) -> StorageResult<()> {
        let offset = (page_id as u64) * (PAGE_SIZE as u64);
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.write_all(&page.data)?;
        Ok(())
    }

    /// Allocate a new page on disk. Returns the new page ID.
    pub fn allocate_page(&mut self) -> StorageResult<PageId> {
        let page_id = self.num_pages;
        let page = Page::new(page_id);
        self.write_page(page_id, &page)?;
        self.num_pages += 1;
        Ok(page_id)
    }

    /// Flush all pending writes to disk.
    pub fn sync(&mut self) -> StorageResult<()> {
        self.file.sync_all()?;
        Ok(())
    }

    /// Number of pages in the file.
    pub fn num_pages(&self) -> u32 {
        self.num_pages
    }

    /// File path.
    pub fn path(&self) -> &str {
        &self.file_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_disk_manager_roundtrip() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();
        let mut dm = DiskManager::open(&path).unwrap();

        let pid = dm.allocate_page().unwrap();
        assert_eq!(pid, 0);

        let mut page = dm.read_page(pid).unwrap();
        page.insert(b"hello disk").unwrap();
        dm.write_page(pid, &page).unwrap();
        dm.sync().unwrap();

        let restored = dm.read_page(pid).unwrap();
        assert_eq!(restored.get(0).unwrap(), b"hello disk");
    }

    #[test]
    fn test_multiple_pages() {
        let tmp = NamedTempFile::new().unwrap();
        let mut dm = DiskManager::open(tmp.path()).unwrap();

        for i in 0..5 {
            let pid = dm.allocate_page().unwrap();
            let mut page = Page::new(pid);
            page.insert(format!("page_{}", i).as_bytes()).unwrap();
            dm.write_page(pid, &page).unwrap();
        }
        dm.sync().unwrap();
        assert_eq!(dm.num_pages(), 5);

        for i in 0..5 {
            let page = dm.read_page(i).unwrap();
            assert_eq!(page.get(0).unwrap(), format!("page_{}", i).as_bytes());
        }
    }
}
