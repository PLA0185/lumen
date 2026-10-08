//! Platform-specific secure credential storage.

#[cfg(not(target_os = "android"))]
pub use keyring::{Entry, Error};
#[cfg(target_os = "android")]
pub use keyring_core::{Entry, Error};

#[cfg(target_os = "android")]
pub fn initialize() -> Result<(), Error> {
    let store = android_native_keyring_store::Store::new()?;
    keyring_core::set_default_store(store);
    Ok(())
}

#[cfg(not(target_os = "android"))]
pub fn initialize() -> Result<(), Error> {
    Ok(())
}
