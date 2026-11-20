//! Bounded newline batches shared by append-only transcript readers.

use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncSeekExt, BufReader};

/// Target batch size; a single larger record remains intact, never truncated.
pub(crate) const BATCH_BYTES: u64 = 4 * 1024 * 1024;

/// Reads at most one batch plus the remainder of its final record.
/// Peak memory depends on batch size and the largest individual native record.
pub(crate) async fn read_chunk(path: &Path, offset: u64) -> Result<Vec<u8>, std::io::Error> {
    let mut file = tokio::fs::File::open(path).await?;
    file.seek(std::io::SeekFrom::Start(offset)).await?;
    let mut reader = BufReader::new(file);
    let mut bytes = Vec::new();
    (&mut reader)
        .take(BATCH_BYTES)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.len() as u64 == BATCH_BYTES && bytes.last() != Some(&b'\n') {
        reader.read_until(b'\n', &mut bytes).await?;
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn batches_preserve_large_records_and_resume_at_exact_boundaries() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("transcript");
        let mut bytes = vec![b'x'; usize::try_from(BATCH_BYTES).unwrap() + 100];
        bytes.extend_from_slice(b"\nnext\n");
        tokio::fs::write(&path, &bytes).await.unwrap();
        let first = read_chunk(&path, 0).await.unwrap();
        assert_eq!(first.len(), bytes.len() - 5);
        assert_eq!(
            read_chunk(&path, first.len() as u64).await.unwrap(),
            b"next\n"
        );
        // Many short records must not turn into one unbounded read.
        let short = b"x\n".repeat(usize::try_from(BATCH_BYTES).unwrap());
        tokio::fs::write(&path, short).await.unwrap();
        assert_eq!(
            read_chunk(&path, 0).await.unwrap().len() as u64,
            BATCH_BYTES
        );
    }
}
