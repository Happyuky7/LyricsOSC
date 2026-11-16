//! Axis output (Windows): sends text through AxisOpenApi.dll so LyricsOSC
//! works as an Axis addon. The DLL is loaded at runtime only when enabled.
#[cfg(windows)]
mod imp {
    type WriteTo = unsafe extern "system" fn(*const u8, bool) -> bool;

    pub struct Axis {
        _lib: libloading::Library,
        f: WriteTo,
    }

    impl Axis {
        pub fn load(path: &std::path::Path) -> Result<Self, String> {
            unsafe {
                let lib = libloading::Library::new(path).map_err(|e| format!("{}: {e}", path.display()))?;
                let f: WriteTo = *lib.get::<WriteTo>(b"AxisOpenApi_writeTo\0").map_err(|e| e.to_string())?;
                Ok(Self { _lib: lib, f })
            }
        }

        /// Non-blocking write (real-time updates).
        /// Returns false if Axis hasn't displayed the previous text yet.
        pub fn send(&self, text: &str) -> bool {
            let mut bytes = text.as_bytes().to_vec();
            bytes.push(0);
            unsafe { (self.f)(bytes.as_ptr(), false) }
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub struct Axis;
    impl Axis {
        pub fn load(_: &std::path::Path) -> Result<Self, String> {
            Err("Axis is only available on Windows".into())
        }
        pub fn send(&self, _: &str) -> bool {
            false
        }
    }
}

pub use imp::Axis;
