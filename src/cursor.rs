// Copyright 2023 Google LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::ops::Range;
use std::ptr::copy_nonoverlapping;

use anyhow::bail;

use crate::btree::parse_btree_leaf_table_cell;
use crate::btree::BtreeInteriorTableCell;
use crate::btree::BtreePageHeader;
use crate::btree::OverflowPage;
use crate::pager::MemPage;
use crate::pager::PageBuffer;
use crate::pager::PageId;
use crate::pager::Pager;

pub struct BtreePayload<'a, 'pager> {
    pager: &'pager Pager,
    local_payload_buffer: PageBuffer<'a>,
    local_payload_range: Range<usize>,
    size: u32,
    overflow: Option<OverflowPage>,
}

impl<'a, 'pager> BtreePayload<'a, 'pager> {
    /// The size of the payload.
    pub fn size(&self) -> u32 {
        self.size
    }

    /// The local payload.
    ///
    /// This may not be the entire payload if there is overflow page.
    pub fn buf(&self) -> &[u8] {
        &self.local_payload_buffer[self.local_payload_range.clone()]
    }

    /// Load the payload into the buffer.
    ///
    /// Returns the number of bytes loaded.
    ///
    /// The offset must be less than the size of the payload.
    ///
    /// # Safety
    ///
    /// The buffer must not be any [MemPage] buffer.
    pub unsafe fn load(&self, offset: u32, buf: &mut [u8]) -> anyhow::Result<usize> {
        if offset >= self.size {
            bail!("offset exceeds payload size");
        }
        let mut n_loaded = 0;
        let mut offset = offset;
        let mut buf = buf;
        let payload = &self.local_payload_buffer[self.local_payload_range.clone()];

        if offset < payload.len() as u32 {
            let local_offset = offset as usize;
            let n = std::cmp::min(payload.len() - local_offset, buf.len());

            // SAFETY: n is less than buf.len() and payload.len().
            // SAFETY: payload and buf do not overlap.
            unsafe {
                copy_nonoverlapping(payload[local_offset..].as_ptr(), buf.as_mut_ptr(), n);
            }
            n_loaded += n;
            offset += n as u32;
            buf = &mut buf[n..];
        }

        let mut cur = payload.len() as u32;
        let mut overflow = self.overflow;
        while !buf.is_empty() && cur < self.size {
            let overflow_page =
                overflow.ok_or_else(|| anyhow::anyhow!("overflow page is not found"))?;
            let page = self.pager.get_page(overflow_page.page_id())?;
            let buffer = page.buffer();
            let (payload, next_overflow) = overflow_page
                .parse(&buffer)
                .map_err(|e| anyhow::anyhow!("parse overflow: {:?}", e))?;
            if offset < cur + payload.len() as u32 {
                let local_offset = (offset - cur) as usize;
                let n = std::cmp::min(payload.len() - local_offset, buf.len());

                // SAFETY: n is less than buf.len() and payload.len().
                // SAFETY: payload and buf do not overlap.
                unsafe {
                    copy_nonoverlapping(payload[local_offset..].as_ptr(), buf.as_mut_ptr(), n);
                }
                n_loaded += n;
                offset += n as u32;
                buf = &mut buf[n..];
            }
            cur += payload.len() as u32;
            overflow = next_overflow;
        }

        Ok(n_loaded)
    }
}

pub struct BtreeCursor<'pager> {
    pager: &'pager Pager,
    usable_size: u32,
    current_page_id: PageId,
    current_page: MemPage,
    idx_cell: u16,
    parent_pages: Vec<(PageId, u16)>,
}

impl<'pager> BtreeCursor<'pager> {
    pub fn new(root_page: PageId, pager: &'pager Pager, usable_size: u32) -> anyhow::Result<Self> {
        Ok(Self {
            pager,
            usable_size,
            current_page_id: root_page,
            current_page: pager.get_page(root_page)?,
            idx_cell: 0,
            parent_pages: Vec::new(),
        })
    }

    pub fn next<'a>(&'a mut self) -> anyhow::Result<Option<BtreePayload<'a, 'pager>>> {
        loop {
            let buffer = self.current_page.buffer();
            let page_header = BtreePageHeader::from_page(&self.current_page, &buffer);
            if !page_header.is_leaf() && self.idx_cell == page_header.n_cells() {
                self.idx_cell += 1;
                let page_id = page_header.right_page_id();
                drop(buffer);
                self.move_to_child(page_id)?;
            } else if self.idx_cell >= page_header.n_cells() {
                drop(buffer);
                if !self.back_to_parent()? {
                    return Ok(None);
                }
            } else if page_header.is_leaf() {
                let (_, size, payload_range, overflow) = parse_btree_leaf_table_cell(
                    &self.current_page,
                    &buffer,
                    self.idx_cell,
                    self.usable_size,
                )
                .map_err(|e| anyhow::anyhow!("parse tree leaf table cell: {:?}", e))?;
                self.idx_cell += 1;
                return Ok(Some(BtreePayload {
                    pager: self.pager,
                    local_payload_buffer: self.current_page.buffer(),
                    local_payload_range: payload_range,
                    size,
                    overflow,
                }));
            } else {
                let cell = BtreeInteriorTableCell::get(&self.current_page, &buffer, self.idx_cell)
                    .map_err(|e| anyhow::anyhow!("get btree interior table cell: {:?}", e))?;
                let page_id = cell.page_id();
                drop(buffer);
                self.move_to_child(page_id)?;
            }
        }
    }

    fn move_to_child(&mut self, page_id: PageId) -> anyhow::Result<()> {
        self.parent_pages
            .push((self.current_page_id, self.idx_cell));
        self.current_page_id = page_id;
        self.current_page = self.pager.get_page(self.current_page_id)?;
        self.idx_cell = 0;
        Ok(())
    }

    fn back_to_parent(&mut self) -> anyhow::Result<bool> {
        let (page_id, idx_cell) = match self.parent_pages.pop() {
            Some((page_id, idx_cell)) => (page_id, idx_cell),
            None => {
                return Ok(false);
            }
        };
        self.current_page_id = page_id;
        self.current_page = self.pager.get_page(self.current_page_id)?;
        self.idx_cell = idx_cell + 1;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_utils::*;

    #[test]
    fn test_btree_cursor_single_page() {
        let file = create_sqlite_database(&[
            "CREATE TABLE example(col);",
            "INSERT INTO example(col) VALUES (0);",
            "INSERT INTO example(col) VALUES (1);",
            "INSERT INTO example(col) VALUES (2);",
        ]);
        let pager = create_pager(file.as_file().try_clone().unwrap()).unwrap();
        let usable_size = load_usable_size(file.as_file()).unwrap();
        let page_id = find_table_page_id("example", &pager, usable_size);

        let mut cursor = BtreeCursor::new(page_id, &pager, usable_size).unwrap();

        let payload = cursor.next().unwrap();
        assert!(payload.is_some());
        let payload = payload.unwrap();
        assert_eq!(payload.buf(), &[2, 8]);
        assert_eq!(payload.size(), payload.buf().len() as u32);
        drop(payload);

        let payload = cursor.next().unwrap();
        assert!(payload.is_some());
        let payload = payload.unwrap();
        assert_eq!(payload.buf(), &[2, 9]);
        assert_eq!(payload.size(), payload.buf().len() as u32);
        drop(payload);

        let payload = cursor.next().unwrap();
        assert!(payload.is_some());
        let payload = payload.unwrap();
        assert_eq!(payload.buf(), &[2, 1, 2]);
        assert_eq!(payload.size(), payload.buf().len() as u32);
        drop(payload);

        assert!(cursor.next().unwrap().is_none());
    }

    #[test]
    fn test_btree_cursor_empty_records() {
        let file = create_sqlite_database(&["CREATE TABLE example(col);"]);
        let pager = create_pager(file.as_file().try_clone().unwrap()).unwrap();
        let usable_size = load_usable_size(file.as_file()).unwrap();
        let page_id = find_table_page_id("example", &pager, usable_size);

        let mut cursor = BtreeCursor::new(page_id, &pager, usable_size).unwrap();
        assert!(cursor.next().unwrap().is_none());
    }

    #[test]
    fn test_btree_cursor_multiple_page() {
        let buf = vec![0; 4000];
        let mut inserts = Vec::new();
        // 1000 byte blob entry occupies 1 page. These 2000 entries introduce
        // 2 level interior pages and 1 leaf page level.
        for i in 0..1000 {
            inserts.push(format!(
                "INSERT INTO example(col,buf) VALUES ({},X'{}');",
                i,
                buffer_to_hex(&buf)
            ));
        }
        for i in 0..1000 {
            inserts.push(format!(
                "INSERT INTO example(col) VALUES ({});",
                i % 100 + 2
            ));
        }
        let mut queries = vec!["CREATE TABLE example(col,buf);"];
        queries.extend(inserts.iter().map(|s| s.as_str()));
        let file = create_sqlite_database(&queries);
        let pager = create_pager(file.as_file().try_clone().unwrap()).unwrap();
        let usable_size = load_usable_size(file.as_file()).unwrap();
        let page_id = find_table_page_id("example", &pager, usable_size);

        let mut cursor = BtreeCursor::new(page_id, &pager, usable_size).unwrap();

        for _ in 0..1000 {
            let payload = cursor.next().unwrap();
            assert!(payload.is_some());
            let payload = payload.unwrap();
            assert!(payload.size() > 4000);
            assert_eq!(payload.size(), payload.buf().len() as u32);
        }
        for i in 0..1000 {
            let payload = cursor.next().unwrap();
            assert!(payload.is_some());
            let payload = payload.unwrap();
            assert_eq!(payload.buf(), &[3, 1, 0, ((i % 100) + 2) as u8]);
            assert_eq!(payload.size(), payload.buf().len() as u32);
        }

        assert!(cursor.next().unwrap().is_none());
    }

    #[test]
    fn test_overflow_payload() {
        let mut queries = vec!["CREATE TABLE example(col);"];
        let mut buf = Vec::with_capacity(10000);
        for i in 0..10000 {
            buf.push((i % 256) as u8);
        }
        let query = format!(
            "INSERT INTO example(col) VALUES (X'{}');",
            buffer_to_hex(&buf)
        );
        queries.push(&query);
        let file = create_sqlite_database(&queries);
        let pager = create_pager(file.as_file().try_clone().unwrap()).unwrap();
        let usable_size = load_usable_size(file.as_file()).unwrap();
        let page_id = find_table_page_id("example", &pager, usable_size);

        let mut cursor = BtreeCursor::new(page_id, &pager, usable_size).unwrap();

        let payload = cursor.next().unwrap();
        assert!(payload.is_some());
        let payload = payload.unwrap();

        assert_eq!(payload.buf().len(), 1820);
        assert_eq!(payload.size(), 10004);

        let mut payload_buf = Vec::with_capacity(10010);
        unsafe {
            payload_buf.set_len(10010);
        }
        let n = unsafe { payload.load(0, &mut payload_buf) }.unwrap();
        assert_eq!(n, 10004);
        assert_eq!(payload_buf[0..4], [0x04, 0x81, 0x9c, 0x2c]);
        assert_eq!(&payload_buf[..payload.buf().len()], payload.buf());
        assert_eq!(payload_buf[4..10004], buf);

        let n = unsafe { payload.load(3000, &mut payload_buf) }.unwrap();
        assert_eq!(n, 7004);
        assert_eq!(payload_buf[..7004], buf[2996..]);

        let n = unsafe { payload.load(104, &mut payload_buf[..100]) }.unwrap();
        assert_eq!(n, 100);
        assert_eq!(payload_buf[..100], buf[100..200]);

        let n = unsafe { payload.load(3000, &mut payload_buf[..100]) }.unwrap();
        assert_eq!(n, 100);
        assert_eq!(payload_buf[..100], buf[2996..3096]);

        let result = unsafe { payload.load(10004, &mut payload_buf) };
        assert!(result.is_err());
    }
}
