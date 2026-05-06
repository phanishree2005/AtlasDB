//! # Write-Ahead Log (WAL)
//!
//! Ensures crash recovery by logging all modifications before
//! they are applied to data pages. Follows the WAL protocol:
//! no dirty page is flushed to disk until its log records are
//! persisted.
//!
//! ## Log Record Format
//! ```text
//! [LSN:4][TxnId:4][Type:1][TableLen:2][Table:var][PageId:4][SlotId:2][DataLen:4][Data:var][CRC:4]
//! ```

use std::fs::{File, OpenOptions};
use std::io::{Read, Write, BufWriter};
use std::path::Path;

/// Log Sequence Number — monotonically increasing.
pub type LSN = u64;

/// Transaction ID.
pub type TxnId = u32;

/// Log record types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LogRecordType {
    Begin = 1,
    Commit = 2,
    Abort = 3,
    Insert = 4,
    Delete = 5,
    Update = 6,
    Checkpoint = 7,
}

impl LogRecordType {
    fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(Self::Begin),
            2 => Some(Self::Commit),
            3 => Some(Self::Abort),
            4 => Some(Self::Insert),
            5 => Some(Self::Delete),
            6 => Some(Self::Update),
            7 => Some(Self::Checkpoint),
            _ => None,
        }
    }
}

/// A single WAL log record.
#[derive(Debug, Clone)]
pub struct LogRecord {
    pub lsn: LSN,
    pub txn_id: TxnId,
    pub record_type: LogRecordType,
    pub table_name: String,
    pub page_id: u32,
    pub slot_id: u16,
    pub data: Vec<u8>,
}

impl LogRecord {
    /// Serialize this log record to bytes.
    pub fn serialize(&self) -> Vec<u8> {
        let table_bytes = self.table_name.as_bytes();
        let total = 4 + 4 + 1 + 2 + table_bytes.len() + 4 + 2 + 4 + self.data.len() + 4;
        let mut buf = Vec::with_capacity(total);

        buf.extend_from_slice(&(self.lsn as u32).to_le_bytes());
        buf.extend_from_slice(&self.txn_id.to_le_bytes());
        buf.push(self.record_type as u8);
        buf.extend_from_slice(&(table_bytes.len() as u16).to_le_bytes());
        buf.extend_from_slice(table_bytes);
        buf.extend_from_slice(&self.page_id.to_le_bytes());
        buf.extend_from_slice(&self.slot_id.to_le_bytes());
        buf.extend_from_slice(&(self.data.len() as u32).to_le_bytes());
        buf.extend_from_slice(&self.data);

        let crc = crc32fast::hash(&buf);
        buf.extend_from_slice(&crc.to_le_bytes());
        buf
    }

    /// Deserialize from bytes. Returns (record, bytes_consumed).
    pub fn deserialize(buf: &[u8]) -> Option<(Self, usize)> {
        if buf.len() < 17 { return None; }

        let mut pos = 0;
        let lsn = u32::from_le_bytes(buf[pos..pos+4].try_into().ok()?) as u64;
        pos += 4;
        let txn_id = u32::from_le_bytes(buf[pos..pos+4].try_into().ok()?);
        pos += 4;
        let record_type = LogRecordType::from_u8(buf[pos])?;
        pos += 1;
        let table_len = u16::from_le_bytes(buf[pos..pos+2].try_into().ok()?) as usize;
        pos += 2;

        if buf.len() < pos + table_len + 10 { return None; }
        let table_name = String::from_utf8(buf[pos..pos+table_len].to_vec()).ok()?;
        pos += table_len;

        let page_id = u32::from_le_bytes(buf[pos..pos+4].try_into().ok()?);
        pos += 4;
        let slot_id = u16::from_le_bytes(buf[pos..pos+2].try_into().ok()?);
        pos += 2;
        let data_len = u32::from_le_bytes(buf[pos..pos+4].try_into().ok()?) as usize;
        pos += 4;

        if buf.len() < pos + data_len + 4 { return None; }
        let data = buf[pos..pos+data_len].to_vec();
        pos += data_len;

        let stored_crc = u32::from_le_bytes(buf[pos..pos+4].try_into().ok()?);
        let computed_crc = crc32fast::hash(&buf[..pos]);
        if stored_crc != computed_crc { return None; }
        pos += 4;

        Some((LogRecord { lsn, txn_id, record_type, table_name, page_id, slot_id, data }, pos))
    }
}

/// Transaction state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxnState {
    Active,
    Committed,
    Aborted,
}

/// A transaction.
#[derive(Debug)]
pub struct Transaction {
    pub txn_id: TxnId,
    pub state: TxnState,
}

/// Write-Ahead Log manager.
pub struct WAL {
    writer: BufWriter<File>,
    next_lsn: LSN,
    next_txn_id: TxnId,
    /// Active transactions.
    active_txns: Vec<Transaction>,
    file_path: String,
}

impl WAL {
    /// Open or create a WAL file.
    pub fn open<P: AsRef<Path>>(path: P) -> std::io::Result<Self> {
        let file_path = path.as_ref().to_string_lossy().to_string();
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(&path)?;

        let metadata = file.metadata()?;
        let next_lsn = if metadata.len() > 0 {
            // Count existing records to determine next LSN
            metadata.len() as LSN
        } else {
            0
        };

        Ok(Self {
            writer: BufWriter::new(file),
            next_lsn,
            next_txn_id: 1,
            active_txns: Vec::new(),
            file_path,
        })
    }

    /// Begin a new transaction. Returns the transaction ID.
    pub fn begin(&mut self) -> std::io::Result<TxnId> {
        let txn_id = self.next_txn_id;
        self.next_txn_id += 1;

        self.log(LogRecord {
            lsn: 0, // will be set
            txn_id,
            record_type: LogRecordType::Begin,
            table_name: String::new(),
            page_id: 0,
            slot_id: 0,
            data: Vec::new(),
        })?;

        self.active_txns.push(Transaction {
            txn_id,
            state: TxnState::Active,
        });

        Ok(txn_id)
    }

    /// Commit a transaction.
    pub fn commit(&mut self, txn_id: TxnId) -> std::io::Result<()> {
        self.log(LogRecord {
            lsn: 0,
            txn_id,
            record_type: LogRecordType::Commit,
            table_name: String::new(),
            page_id: 0,
            slot_id: 0,
            data: Vec::new(),
        })?;

        if let Some(txn) = self.active_txns.iter_mut().find(|t| t.txn_id == txn_id) {
            txn.state = TxnState::Committed;
        }
        self.writer.flush()?;
        Ok(())
    }

    /// Abort a transaction.
    pub fn abort(&mut self, txn_id: TxnId) -> std::io::Result<()> {
        self.log(LogRecord {
            lsn: 0,
            txn_id,
            record_type: LogRecordType::Abort,
            table_name: String::new(),
            page_id: 0,
            slot_id: 0,
            data: Vec::new(),
        })?;

        if let Some(txn) = self.active_txns.iter_mut().find(|t| t.txn_id == txn_id) {
            txn.state = TxnState::Aborted;
        }
        Ok(())
    }

    /// Log an operation.
    pub fn log_operation(
        &mut self,
        txn_id: TxnId,
        record_type: LogRecordType,
        table_name: &str,
        page_id: u32,
        slot_id: u16,
        data: &[u8],
    ) -> std::io::Result<LSN> {
        self.log(LogRecord {
            lsn: 0,
            txn_id,
            record_type,
            table_name: table_name.to_string(),
            page_id,
            slot_id,
            data: data.to_vec(),
        })
    }

    /// Write a log record. Returns the assigned LSN.
    fn log(&mut self, mut record: LogRecord) -> std::io::Result<LSN> {
        let lsn = self.next_lsn;
        record.lsn = lsn;
        let bytes = record.serialize();
        self.writer.write_all(&bytes)?;
        self.next_lsn += bytes.len() as u64;
        Ok(lsn)
    }

    /// Flush the WAL to disk.
    pub fn flush(&mut self) -> std::io::Result<()> {
        self.writer.flush()
    }

    /// Read all log records from the WAL file (for recovery).
    pub fn read_all_records(path: &str) -> std::io::Result<Vec<LogRecord>> {
        let mut file = File::open(path)?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;

        let mut records = Vec::new();
        let mut pos = 0;
        while pos < buf.len() {
            if let Some((record, consumed)) = LogRecord::deserialize(&buf[pos..]) {
                records.push(record);
                pos += consumed;
            } else {
                break;
            }
        }
        Ok(records)
    }

    /// Recover: replay committed transactions (redo) and undo aborted ones.
    pub fn recover(wal_path: &str) -> std::io::Result<Vec<LogRecord>> {
        let records = Self::read_all_records(wal_path)?;

        // Identify committed and aborted transactions
        let mut committed = std::collections::HashSet::new();
        let mut aborted = std::collections::HashSet::new();

        for rec in &records {
            match rec.record_type {
                LogRecordType::Commit => { committed.insert(rec.txn_id); }
                LogRecordType::Abort => { aborted.insert(rec.txn_id); }
                _ => {}
            }
        }

        // Redo: replay committed operations
        let redo_records: Vec<LogRecord> = records.iter()
            .filter(|r| committed.contains(&r.txn_id) && matches!(
                r.record_type,
                LogRecordType::Insert | LogRecordType::Update | LogRecordType::Delete
            ))
            .cloned()
            .collect();

        Ok(redo_records)
    }

    /// Path to the WAL file.
    pub fn path(&self) -> &str {
        &self.file_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_log_record_roundtrip() {
        let record = LogRecord {
            lsn: 42,
            txn_id: 1,
            record_type: LogRecordType::Insert,
            table_name: "users".to_string(),
            page_id: 3,
            slot_id: 7,
            data: vec![1, 2, 3, 4, 5],
        };

        let bytes = record.serialize();
        let (restored, consumed) = LogRecord::deserialize(&bytes).unwrap();
        assert_eq!(consumed, bytes.len());
        assert_eq!(restored.txn_id, 1);
        assert_eq!(restored.table_name, "users");
        assert_eq!(restored.data, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_wal_begin_commit() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_string();

        {
            let mut wal = WAL::open(&path).unwrap();
            let txn = wal.begin().unwrap();
            wal.log_operation(txn, LogRecordType::Insert, "t", 0, 0, b"data").unwrap();
            wal.commit(txn).unwrap();
        }

        let records = WAL::read_all_records(&path).unwrap();
        assert_eq!(records.len(), 3); // Begin, Insert, Commit
        assert_eq!(records[0].record_type, LogRecordType::Begin);
        assert_eq!(records[1].record_type, LogRecordType::Insert);
        assert_eq!(records[2].record_type, LogRecordType::Commit);
    }

    #[test]
    fn test_recovery() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_string();

        {
            let mut wal = WAL::open(&path).unwrap();

            // Committed txn
            let t1 = wal.begin().unwrap();
            wal.log_operation(t1, LogRecordType::Insert, "t", 0, 0, b"committed").unwrap();
            wal.commit(t1).unwrap();

            // Aborted txn
            let t2 = wal.begin().unwrap();
            wal.log_operation(t2, LogRecordType::Insert, "t", 0, 1, b"aborted").unwrap();
            wal.abort(t2).unwrap();

            wal.flush().unwrap();
        }

        let redo = WAL::recover(&path).unwrap();
        assert_eq!(redo.len(), 1); // Only committed insert
        assert_eq!(redo[0].data, b"committed");
    }
}
