//! Windows: the monitor-configuration API, which does the protocol for us.
//!
//! This is the platform that justifies where the seam is. `dxva2.dll` takes a
//! VCP code and a value and handles framing, checksums, timing and retries
//! inside the driver stack — so this backend implements [`VcpBackend`]
//! directly and never touches [`crate::Ddc`] or a packet. A seam one layer
//! lower would have handed Windows a packet layer to route around.
//!
//! What it costs is identification. `dxva2` describes a monitor as whatever
//! the driver called it, which is very often "Generic PnP Monitor" for every
//! panel on the desk. The EDID is in the registry, under SetupAPI, and reading
//! it is a separate piece of work — so displays here are numbered rather than
//! named, and that is a real gap rather than a temporary one.

use std::ffi::c_void;

use roadie_ddc::{Capabilities, Feature, Value};
use windows_sys::Win32::Devices::Display::{
    CapabilitiesRequestAndCapabilitiesReply, DestroyPhysicalMonitor, GetCapabilitiesStringLength,
    GetNumberOfPhysicalMonitorsFromHMONITOR, GetPhysicalMonitorsFromHMONITOR,
    GetVCPFeatureAndVCPFeatureReply, PHYSICAL_MONITOR, SaveCurrentSettings, SetVCPFeature,
};
use windows_sys::Win32::Foundation::{LPARAM, RECT, TRUE};
use windows_sys::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};

use crate::backend::{DisplayError, VcpBackend, boxed};
use crate::{Display, DisplayId};

/// One physical monitor, as `dxva2` hands it out.
///
/// The handle is kept as an `isize` rather than as the raw pointer type it
/// arrives as, so that nothing in this struct is a pointer whose provenance a
/// reader has to reason about. It is a kernel handle, not an address this
/// process may dereference, and storing it as a number says so.
#[derive(Debug)]
pub(crate) struct PhysicalMonitor {
    handle: isize,
    name: String,
}

impl PhysicalMonitor {
    /// The handle, back in the shape the API wants.
    fn raw(&self) -> *mut c_void {
        self.handle as *mut c_void
    }

    /// Shape a failed call as an error, using the OS's own explanation.
    fn failed(&self, what: &str) -> DisplayError {
        DisplayError::Transport {
            name: self.name.clone(),
            reason: format!("{what}: {}", std::io::Error::last_os_error()),
            // `dxva2` has already retried inside the driver stack. A failure
            // that reaches here is one the OS has stopped believing in, and
            // asking again would only spend the timeout twice.
            retryable: false,
        }
    }
}

impl Drop for PhysicalMonitor {
    #[expect(
        unsafe_code,
        reason = "DestroyPhysicalMonitor is the release half of GetPhysicalMonitorsFromHMONITOR"
    )]
    fn drop(&mut self) {
        // SAFETY: `handle` came from `GetPhysicalMonitorsFromHMONITOR` and has
        // not been destroyed before now — this type is the only owner and is
        // not `Clone`. Destroying it exactly once is what the API asks for.
        unsafe {
            DestroyPhysicalMonitor(self.raw());
        }
    }
}

impl VcpBackend for PhysicalMonitor {
    fn name(&self) -> String {
        self.name.clone()
    }

    #[expect(unsafe_code, reason = "the dxva2 monitor-configuration API is a C API")]
    fn get(&mut self, feature: Feature) -> Result<Value, DisplayError> {
        let mut kind = 0;
        let mut current = 0_u32;
        let mut maximum = 0_u32;
        // SAFETY: the handle is live for the lifetime of `self`, and the three
        // out-parameters are stack locals that outlive the call.
        let ok = unsafe {
            GetVCPFeatureAndVCPFeatureReply(
                self.raw(),
                feature.code(),
                &raw mut kind,
                &raw mut current,
                &raw mut maximum,
            )
        };
        if ok != TRUE {
            return Err(self.failed("reading a feature"));
        }
        // The API reports both as 32-bit, but MCCS values are 16-bit and a
        // driver that returns more than that has said something impossible.
        // Saturating is the forgiving reading: a clamped maximum still lets a
        // percentage be computed, where a hard error would take the whole
        // display away over one bad field.
        Ok(Value {
            current: u16::try_from(current).unwrap_or(u16::MAX),
            maximum: u16::try_from(maximum).unwrap_or(u16::MAX),
        })
    }

    #[expect(unsafe_code, reason = "the dxva2 monitor-configuration API is a C API")]
    fn set(&mut self, feature: Feature, value: u16) -> Result<(), DisplayError> {
        // SAFETY: the handle is live for the lifetime of `self`.
        let ok = unsafe { SetVCPFeature(self.raw(), feature.code(), u32::from(value)) };
        if ok != TRUE {
            return Err(self.failed("writing a feature"));
        }
        Ok(())
    }

    #[expect(unsafe_code, reason = "the dxva2 monitor-configuration API is a C API")]
    fn capabilities(&mut self) -> Result<Capabilities, DisplayError> {
        let mut length = 0_u32;
        // SAFETY: the handle is live and `length` is a stack local.
        let ok = unsafe { GetCapabilitiesStringLength(self.raw(), &raw mut length) };
        if ok != TRUE {
            return Err(self.failed("asking how long the capability string is"));
        }

        let length = length as usize;
        let mut buffer = vec![0_u8; length];
        // SAFETY: the handle is live, and the buffer has exactly the `length`
        // bytes the previous call asked us to provide.
        let ok = unsafe {
            CapabilitiesRequestAndCapabilitiesReply(
                self.raw(),
                buffer.as_mut_ptr(),
                // The length is the driver's own answer; it cannot be wider
                // than the u32 it arrived in.
                u32::try_from(length).unwrap_or(u32::MAX),
            )
        };
        if ok != TRUE {
            return Err(self.failed("reading the capability string"));
        }

        // The string is NUL-terminated and the length includes the terminator.
        // Leaving it on would put a zero byte inside a parsed model name,
        // which reaches a screen reader as a name followed by silence.
        let text = buffer.split(|byte| *byte == 0).next().unwrap_or(&buffer);
        Capabilities::parse(text).map_err(|source| DisplayError::Capabilities {
            name: self.name.clone(),
            source,
        })
    }

    #[expect(unsafe_code, reason = "the dxva2 monitor-configuration API is a C API")]
    fn save_settings(&mut self) -> Result<(), DisplayError> {
        // SAFETY: the handle is live for the lifetime of `self`.
        let ok = unsafe { SaveCurrentSettings(self.raw()) };
        if ok != TRUE {
            return Err(self.failed("saving the current settings"));
        }
        Ok(())
    }
}

/// Collects the `HMONITOR`s `EnumDisplayMonitors` calls back with.
///
/// The callback is a plain C function pointer with no captures, so the vector
/// it fills travels through the `LPARAM` the API provides for exactly this.
#[expect(
    unsafe_code,
    reason = "EnumDisplayMonitors takes a C callback and an untyped user pointer"
)]
unsafe extern "system" fn collect(
    monitor: HMONITOR,
    _context: HDC,
    _bounds: *mut RECT,
    data: LPARAM,
) -> windows_sys::core::BOOL {
    // SAFETY: `data` is the `LPARAM` passed to `EnumDisplayMonitors` below,
    // which is a pointer to the `Vec` living on that function's stack for the
    // whole of the enumeration. The API calls this synchronously and on the
    // same thread, so the borrow cannot overlap with any other.
    let monitors = unsafe { &mut *(data as *mut Vec<HMONITOR>) };
    monitors.push(monitor);
    TRUE
}

/// Every physical monitor Windows can offer a control channel to.
///
/// Fallible in signature and infallible in fact: nothing here can fail in a
/// way worth reporting, since a display adapter with no controllable monitor
/// behind it is an ordinary state rather than an error. The `Result` is the
/// shape [`crate::enumerate`] dispatches to on all three platforms, and on
/// Linux reading the kernel's display directory genuinely can fail.
#[expect(
    clippy::unnecessary_wraps,
    reason = "one signature across three platforms; the Linux one can fail"
)]
#[expect(
    unsafe_code,
    reason = "the display enumeration and monitor-configuration APIs are C APIs"
)]
pub(crate) fn enumerate() -> Result<Vec<Display>, DisplayError> {
    let mut handles: Vec<HMONITOR> = Vec::new();
    // SAFETY: `collect` matches the callback signature the API expects, and
    // the pointer handed to it borrows `handles`, which outlives the call.
    unsafe {
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            Some(collect),
            (&raw mut handles) as LPARAM,
        );
    }

    let mut displays = Vec::new();
    for handle in handles {
        let mut count = 0_u32;
        // SAFETY: `handle` came from the enumeration above and `count` is a
        // stack local.
        let ok = unsafe { GetNumberOfPhysicalMonitorsFromHMONITOR(handle, &raw mut count) };
        if ok != TRUE || count == 0 {
            // A display adapter with no controllable monitor behind it. Not an
            // error: laptop panels and virtual displays look exactly like this.
            continue;
        }

        let mut physical = vec![
            PHYSICAL_MONITOR {
                hPhysicalMonitor: std::ptr::null_mut(),
                szPhysicalMonitorDescription: [0; 128],
            };
            count as usize
        ];
        // SAFETY: the vector has exactly `count` elements, which is the size
        // the previous call reported.
        let ok = unsafe { GetPhysicalMonitorsFromHMONITOR(handle, count, physical.as_mut_ptr()) };
        if ok != TRUE {
            continue;
        }

        for monitor in physical {
            // `PHYSICAL_MONITOR` is a packed struct, so its fields are copied
            // out before being read. Borrowing one in place would be an
            // unaligned reference, which is undefined behaviour even unread.
            let description = monitor.szPhysicalMonitorDescription;
            let handle = monitor.hPhysicalMonitor as isize;
            let name = String::from_utf16_lossy(&description)
                .trim_end_matches('\0')
                .trim()
                .to_owned();
            let index = displays.len();
            let name = if name.is_empty() {
                format!("display {}", index + 1)
            } else {
                name
            };
            let backend = PhysicalMonitor { handle, name };
            // Numbered, because `dxva2` very often calls every panel on the
            // desk the same thing. Without the EDID there is nothing better to
            // distinguish them by, and a list of three identical names would
            // be worse than a list of three numbers.
            displays.push(Display::new(
                DisplayId::new(format!("display-{}", index + 1)),
                None,
                boxed(backend),
            ));
        }
    }
    Ok(displays)
}
