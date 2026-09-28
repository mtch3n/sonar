//! Hashes of what's in files, so work done on a file is kept when it's moved,
//! copied, touched or indexed again.

use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::Path,
};

/// Files up to this size are hashed whole.
const WHOLE: u64 = 64 * 1024 * 1024;
/// Bigger files, mostly videos, are hashed by their size and this much from their
/// start, middle and end, which tells files apart without reading gigabytes.
const SAMPLE: u64 = 1024 * 1024;

pub(crate) type Hash = [u8; 32];

/// The hash of a file's content, or `None` when it can't be read.
pub(crate) fn of_file(path: &Path) -> Option<Hash> {
    hash(path).ok()
}

fn hash(path: &Path) -> io::Result<Hash> {
    let mut file = File::open(path)?;
    let size = file.metadata()?.len();
    let mut hasher = blake3::Hasher::new();
    if size <= WHOLE {
        hasher.update_reader(&mut file)?;
    } else {
        hasher.update(&size.to_le_bytes());
        let mut buffer = vec![0; SAMPLE as usize];
        for start in [0, size / 2 - SAMPLE / 2, size - SAMPLE] {
            file.seek(SeekFrom::Start(start))?;
            file.read_exact(&mut buffer)?;
            hasher.update(&buffer);
        }
    }
    Ok(*hasher.finalize().as_bytes())
}

#[cfg(test)]
mod tests {
    use std::{fs, io::Write};

    use super::*;

    #[test]
    fn same_content_same_hash() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a"), "receipt").unwrap();
        fs::write(dir.path().join("b"), "receipt").unwrap();
        fs::write(dir.path().join("c"), "invoice").unwrap();
        let hash = |name: &str| of_file(&dir.path().join(name)).unwrap();
        assert_eq!(hash("a"), hash("b"));
        assert_ne!(hash("a"), hash("c"));
        assert_eq!(of_file(&dir.path().join("missing")), None);
    }

    #[test]
    fn samples_big_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big");
        let mut file = File::create(&path).unwrap();
        file.set_len(WHOLE + 10 * SAMPLE).unwrap();
        let before = of_file(&path).unwrap();
        // A change between the samples isn't seen; one inside them is.
        file.seek(SeekFrom::Start(2 * SAMPLE)).unwrap();
        file.write_all(b"x").unwrap();
        assert_eq!(of_file(&path).unwrap(), before);
        file.seek(SeekFrom::Start(10)).unwrap();
        file.write_all(b"x").unwrap();
        assert_ne!(of_file(&path).unwrap(), before);
    }
}
