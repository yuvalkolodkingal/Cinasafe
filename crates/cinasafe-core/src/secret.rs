//! Secret material in memory: zeroized on drop, and kept out of swap where the
//! OS allows (`mlock`/`VirtualLock`, best-effort).

use std::fmt;

use zeroize::{Zeroize, Zeroizing};

/// A password as typed. Zeroized when dropped.
pub type Password = Zeroizing<String>;

/// A 32-byte key on the heap, page-locked when possible, zeroized on drop.
pub struct SecretKey {
    bytes: Box<[u8; 32]>,
    locked: bool,
}

impl SecretKey {
    pub fn new(bytes: [u8; 32]) -> SecretKey {
        let mut src = Zeroizing::new(bytes);
        let mut boxed = Box::new([0u8; 32]);
        // Lock before copying the secret in, so it never sits in an unlocked page.
        // SAFETY: the pointer and length describe the boxed array, which stays
        // allocated (and at the same address) until Drop unlocks it.
        let locked = unsafe { memsec::mlock(boxed.as_mut_ptr(), boxed.len()) };
        boxed.copy_from_slice(src.as_ref());
        src.zeroize();
        SecretKey { bytes: boxed, locked }
    }

    pub fn expose(&self) -> &[u8; 32] {
        &self.bytes
    }

    /// Whether the OS agreed to keep these pages out of swap.
    pub fn is_locked(&self) -> bool {
        self.locked
    }
}

impl Clone for SecretKey {
    fn clone(&self) -> SecretKey {
        SecretKey::new(*self.bytes)
    }
}

impl Drop for SecretKey {
    fn drop(&mut self) {
        self.bytes.zeroize();
        if self.locked {
            // SAFETY: same allocation that was locked in `new`.
            unsafe { memsec::munlock(self.bytes.as_mut_ptr(), self.bytes.len()) };
        }
    }
}

impl fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretKey(..)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holds_and_clones() {
        let k = SecretKey::new([7u8; 32]);
        assert_eq!(k.expose(), &[7u8; 32]);
        let c = k.clone();
        drop(k);
        assert_eq!(c.expose(), &[7u8; 32]);
        assert_eq!(format!("{c:?}"), "SecretKey(..)");
    }
}
