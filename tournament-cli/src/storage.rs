//! Where a [`Session`](crate::session::Session) saves its competition and loads it from.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// Holds saved competitions, each as bytes under a name.
pub trait Storage {
    /// The bytes saved under `name`.
    ///
    /// # Errors
    /// Returns an error of kind [`io::ErrorKind::NotFound`] if nothing is saved under `name`, and
    /// another error if what is saved there cannot be read.
    fn read(&self, name: &str) -> io::Result<Vec<u8>>;

    /// Saves `bytes` under `name`, replacing what was saved there.
    ///
    /// # Errors
    /// Returns an error if the bytes cannot be saved, in which case what was saved under `name`
    /// before is still there.
    fn write(&mut self, name: &str, bytes: &[u8]) -> io::Result<()>;
}

/// Saves to files, a name being the path of its file.
pub struct FileStorage;

impl Storage for FileStorage {
    fn read(&self, name: &str) -> io::Result<Vec<u8>> {
        fs::read(name)
    }

    fn write(&mut self, name: &str, bytes: &[u8]) -> io::Result<()> {
        // writing to a sibling first and moving that over the file means a failed write never
        // destroys the previous save
        let mut temporary = Path::new(name).as_os_str().to_owned();
        temporary.push(".tmp");
        let temporary = PathBuf::from(temporary);
        fs::write(&temporary, bytes)?;
        fs::rename(&temporary, name)
    }
}

#[cfg(test)]
pub(crate) use memory::MemoryStorage;

#[cfg(test)]
mod memory {
    use std::{cell::RefCell, collections::HashMap, io, rc::Rc};

    use super::Storage;

    /// Saves in memory. Its clones share what is saved, so that a test can load in one session
    /// what another one saved.
    #[derive(Clone, Default)]
    pub(crate) struct MemoryStorage(Rc<RefCell<HashMap<String, Vec<u8>>>>);

    impl Storage for MemoryStorage {
        fn read(&self, name: &str) -> io::Result<Vec<u8>> {
            self.0
                .borrow()
                .get(name)
                .cloned()
                .ok_or_else(|| io::ErrorKind::NotFound.into())
        }

        fn write(&mut self, name: &str, bytes: &[u8]) -> io::Result<()> {
            self.0.borrow_mut().insert(name.to_owned(), bytes.to_vec());
            Ok(())
        }
    }
}

// files only exist off the browser
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn a_file_reads_back_what_was_written_to_it() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("cup.json");
        let name = path.to_str().unwrap();

        FileStorage.write(name, b"first").unwrap();
        FileStorage.write(name, b"second").unwrap();

        assert_eq!(FileStorage.read(name).unwrap(), b"second");
    }

    #[test]
    fn a_missing_file_is_not_found() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("cup.json");

        let error = FileStorage.read(path.to_str().unwrap()).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn a_failed_write_keeps_the_previous_save() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("cup.json");
        let name = path.to_str().unwrap();
        FileStorage.write(name, b"saved").unwrap();
        // the temporary sibling cannot be written if a directory is in its way
        fs::create_dir(directory.path().join("cup.json.tmp")).unwrap();

        assert!(FileStorage.write(name, b"lost").is_err());
        assert_eq!(FileStorage.read(name).unwrap(), b"saved");
    }
}
