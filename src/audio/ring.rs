//! Helpers for pushing data into rtrb SPSC ring buffers from audio callbacks.
//!
//! These never block: on overrun, we write what fits and return the dropped
//! count so the caller can log/meter it.

use rtrb::Producer;
use rtrb::chunks::ChunkError;

/// Push as many samples as will fit. Returns the number written.
pub fn push_or_drop<T: Copy>(prod: &mut Producer<T>, samples: &[T]) -> usize {
    if samples.is_empty() {
        return 0;
    }
    match prod.write_chunk_uninit(samples.len()) {
        Ok(chunk) => chunk.fill_from_iter(samples.iter().copied()),
        Err(ChunkError::TooFewSlots(0)) => 0,
        Err(ChunkError::TooFewSlots(n)) => {
            // Some space — partial write.
            match prod.write_chunk_uninit(n) {
                Ok(chunk) => chunk.fill_from_iter(samples.iter().copied().take(n)),
                Err(_) => 0,
            }
        }
    }
}
