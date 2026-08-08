use crate::client::CompactBlockData;
use crate::{Error, Result};

/// A contiguous compact-block subchunk bounded by encoded wire bytes.
pub(crate) struct OrderedBlockChunk {
    pub(crate) blocks: Vec<CompactBlockData>,
    pub(crate) encoded_block_bytes: Vec<u64>,
    pub(crate) encoded_bytes: u64,
}

impl OrderedBlockChunk {
    #[cfg(test)]
    pub(crate) fn start_height(&self) -> Option<u64> {
        self.blocks.first().map(|block| block.height)
    }

    #[cfg(test)]
    pub(crate) fn end_height(&self) -> Option<u64> {
        self.blocks.last().map(|block| block.height)
    }
}

/// Strictly ordered assembler for one logical block range.
///
/// A single oversized block is emitted by itself. Every other emitted chunk is
/// at most `max_encoded_bytes`, and an interrupted stream can flush its already
/// validated prefix before the next endpoint resumes at `next_height()`.
pub(crate) struct OrderedBlockAssembler {
    requested_end_exclusive: u64,
    next_height: u64,
    max_encoded_bytes: u64,
    max_blocks: u64,
    blocks: Vec<CompactBlockData>,
    encoded_block_bytes: Vec<u64>,
    encoded_bytes: u64,
    previous_hash: Option<Vec<u8>>,
}

impl OrderedBlockAssembler {
    #[cfg(test)]
    pub(crate) fn new(start: u64, end_exclusive: u64, max_encoded_bytes: u64) -> Result<Self> {
        Self::with_limits(start, end_exclusive, max_encoded_bytes, u64::MAX)
    }

    pub(crate) fn with_limits(
        start: u64,
        end_exclusive: u64,
        max_encoded_bytes: u64,
        max_blocks: u64,
    ) -> Result<Self> {
        if start > end_exclusive {
            return Err(Error::Sync(format!(
                "invalid ordered stream range {}..{}",
                start, end_exclusive
            )));
        }
        Ok(Self {
            requested_end_exclusive: end_exclusive,
            next_height: start,
            max_encoded_bytes: max_encoded_bytes.max(1),
            max_blocks: max_blocks.max(1),
            blocks: Vec::new(),
            encoded_block_bytes: Vec::new(),
            encoded_bytes: 0,
            previous_hash: None,
        })
    }

    pub(crate) fn next_height(&self) -> u64 {
        self.next_height
    }

    pub(crate) fn is_complete(&self) -> bool {
        self.next_height == self.requested_end_exclusive
    }

    /// Applies a new block ceiling only between emitted chunks.
    pub(crate) fn set_next_chunk_max_blocks(&mut self, max_blocks: u64) {
        if self.blocks.is_empty() {
            self.max_blocks = max_blocks.max(1);
        }
    }

    pub(crate) fn push(
        &mut self,
        block: CompactBlockData,
        encoded_bytes: u64,
    ) -> Result<Option<OrderedBlockChunk>> {
        if block.height != self.next_height {
            return Err(Error::Sync(format!(
                "compact block stream expected height {}, received {}",
                self.next_height, block.height
            )));
        }
        if block.height >= self.requested_end_exclusive {
            return Err(Error::Sync(format!(
                "compact block stream returned height {} outside requested end {}",
                block.height, self.requested_end_exclusive
            )));
        }
        if block.hash.len() != 32 || block.prev_hash.len() != 32 {
            return Err(Error::Sync(format!(
                "compact block {} has malformed hash lengths hash={} prev_hash={}",
                block.height,
                block.hash.len(),
                block.prev_hash.len()
            )));
        }
        if let Some(previous_hash) = self.previous_hash.as_deref() {
            if block.prev_hash.as_slice() != previous_hash {
                return Err(Error::Sync(format!(
                    "compact block stream disconnected at height {}",
                    block.height
                )));
            }
        }

        let encoded_bytes = encoded_bytes.max(1);
        let completed = if !self.blocks.is_empty()
            && self.encoded_bytes.saturating_add(encoded_bytes) > self.max_encoded_bytes
        {
            self.take_partial()
        } else {
            None
        };

        self.previous_hash = Some(block.hash.clone());
        self.next_height = self.next_height.saturating_add(1);
        self.encoded_bytes = self.encoded_bytes.saturating_add(encoded_bytes);
        self.encoded_block_bytes.push(encoded_bytes);
        self.blocks.push(block);

        if completed.is_some() {
            Ok(completed)
        } else if self.blocks.len() as u64 >= self.max_blocks
            || self.encoded_bytes >= self.max_encoded_bytes
        {
            Ok(self.take_partial())
        } else {
            Ok(None)
        }
    }

    pub(crate) fn take_partial(&mut self) -> Option<OrderedBlockChunk> {
        if self.blocks.is_empty() {
            return None;
        }
        Some(OrderedBlockChunk {
            blocks: std::mem::take(&mut self.blocks),
            encoded_block_bytes: std::mem::take(&mut self.encoded_block_bytes),
            encoded_bytes: std::mem::take(&mut self.encoded_bytes),
        })
    }

    pub(crate) fn finish(mut self) -> Result<Option<OrderedBlockChunk>> {
        if !self.is_complete() {
            return Err(Error::Sync(format!(
                "compact block stream ended at {}, expected {}",
                self.next_height, self.requested_end_exclusive
            )));
        }
        Ok(self.take_partial())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(height: u64, previous_hash: Vec<u8>) -> CompactBlockData {
        CompactBlockData {
            proto_version: 1,
            height,
            hash: vec![height as u8; 32],
            prev_hash: previous_hash,
            time: height as u32,
            header: Vec::new(),
            transactions: Vec::new(),
        }
    }

    #[test]
    fn splits_before_exceeding_the_actual_byte_limit() {
        let mut assembler = OrderedBlockAssembler::new(10, 13, 100).unwrap();
        assert!(assembler
            .push(block(10, vec![0; 32]), 60)
            .unwrap()
            .is_none());
        let first = assembler
            .push(block(11, vec![10; 32]), 60)
            .unwrap()
            .expect("first bounded chunk");
        assert_eq!(first.start_height(), Some(10));
        assert_eq!(first.end_height(), Some(10));
        assert_eq!(first.encoded_bytes, 60);
        let second = assembler
            .push(block(12, vec![11; 32]), 40)
            .unwrap()
            .expect("exactly byte-bounded chunk");
        assert_eq!(second.start_height(), Some(11));
        assert_eq!(second.end_height(), Some(12));
        assert_eq!(second.encoded_bytes, 100);
        assert!(assembler.finish().unwrap().is_none());
    }

    #[test]
    fn interruption_flushes_a_validated_prefix_for_resume() {
        let mut assembler = OrderedBlockAssembler::new(20, 23, 1_000).unwrap();
        assembler.push(block(20, vec![0; 32]), 50).unwrap();
        assembler.push(block(21, vec![20; 32]), 50).unwrap();
        let prefix = assembler.take_partial().expect("validated prefix");
        assert_eq!(prefix.end_height(), Some(21));
        assert_eq!(assembler.next_height(), 22);
        assembler.push(block(22, vec![21; 32]), 50).unwrap();
        assert!(assembler.finish().is_ok());
    }

    #[test]
    fn rejects_gaps_duplicates_and_disconnected_hashes() {
        let mut gap = OrderedBlockAssembler::new(30, 32, 100).unwrap();
        assert!(gap.push(block(31, vec![0; 32]), 10).is_err());

        let mut disconnected = OrderedBlockAssembler::new(40, 42, 100).unwrap();
        disconnected.push(block(40, vec![0; 32]), 10).unwrap();
        assert!(disconnected.push(block(41, vec![99; 32]), 10).is_err());
    }

    #[test]
    fn emits_one_oversized_block_without_violating_order() {
        let mut assembler = OrderedBlockAssembler::new(50, 51, 100).unwrap();
        let chunk = assembler
            .push(block(50, vec![0; 32]), 1_000)
            .unwrap()
            .expect("oversized chunk");
        assert_eq!(chunk.encoded_bytes, 1_000);
        assert_eq!(chunk.start_height(), Some(50));
        assert_eq!(chunk.end_height(), Some(50));
        assert!(assembler.finish().unwrap().is_none());
    }

    #[test]
    fn fixed_network_segments_are_independent_of_the_byte_limit() {
        let mut assembler = OrderedBlockAssembler::with_limits(60, 65, 1_000, 2).unwrap();
        assembler.push(block(60, vec![0; 32]), 10).unwrap();
        let first = assembler
            .push(block(61, vec![60; 32]), 10)
            .unwrap()
            .expect("two-block network segment");
        assert_eq!(first.start_height(), Some(60));
        assert_eq!(first.end_height(), Some(61));
        assert_eq!(first.encoded_block_bytes, vec![10, 10]);

        assembler.push(block(62, vec![61; 32]), 10).unwrap();
        let second = assembler
            .push(block(63, vec![62; 32]), 10)
            .unwrap()
            .expect("second two-block network segment");
        assert_eq!(second.start_height(), Some(62));
        assert_eq!(second.end_height(), Some(63));
        assembler.push(block(64, vec![63; 32]), 10).unwrap();
        assert_eq!(
            assembler.finish().unwrap().unwrap().start_height(),
            Some(64)
        );
    }

    #[test]
    fn adaptive_block_limit_changes_only_between_chunks() {
        let mut assembler = OrderedBlockAssembler::with_limits(70, 76, 1_000, 2).unwrap();
        assembler.push(block(70, vec![0; 32]), 10).unwrap();
        assembler.set_next_chunk_max_blocks(4);
        let first = assembler
            .push(block(71, vec![70; 32]), 10)
            .unwrap()
            .expect("original two-block chunk");
        assert_eq!(first.blocks.len(), 2);

        assembler.set_next_chunk_max_blocks(4);
        assembler.push(block(72, vec![71; 32]), 10).unwrap();
        assembler.push(block(73, vec![72; 32]), 10).unwrap();
        assembler.push(block(74, vec![73; 32]), 10).unwrap();
        let second = assembler
            .push(block(75, vec![74; 32]), 10)
            .unwrap()
            .expect("new four-block chunk");
        assert_eq!(second.blocks.len(), 4);
    }
}
