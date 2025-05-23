/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::io;
use std::path::Path;

use fs::File;
use fs_err as fs;

use crate::errors::IOContext;
use crate::file::open;

/// RAII lock on a filesystem path.
#[derive(Debug)]
pub struct PathLock {
    file: File,
}

impl PathLock {
    /// Take an exclusive lock on `path`. The lock file will be created on
    /// demand.
    pub fn exclusive<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let file = open(path.as_ref(), "wc").io_context("lock file")?;
        fs2::FileExt::lock_exclusive(file.file())
            .path_context("error locking file", path.as_ref())?;
        Ok(PathLock { file })
    }

    pub fn as_file(&self) -> &File {
        &self.file
    }
}

impl Drop for PathLock {
    fn drop(&mut self) {
        fs2::FileExt::unlock(self.file.file()).expect("unlock");
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::channel;
    use std::thread;

    use super::*;

    #[test]
    fn test_path_lock() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("a");
        let (tx, rx) = channel();
        const N: usize = 50;
        let threads: Vec<_> = (0..N)
            .map(|i| {
                let path = path.clone();
                let tx = tx.clone();
                thread::spawn(move || {
                    // Write 2 values that are the same, protected by the lock.
                    let _locked = PathLock::exclusive(&path);
                    tx.send(i).unwrap();
                    tx.send(i).unwrap();
                })
            })
            .collect();

        for thread in threads {
            thread.join().expect("joined");
        }

        for _ in 0..N {
            // Read 2 values. They should be the same.
            let v1 = rx.recv().unwrap();
            let v2 = rx.recv().unwrap();
            assert_eq!(v1, v2);
        }

        Ok(())
    }
}
