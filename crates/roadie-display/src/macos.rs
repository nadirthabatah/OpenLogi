//! macOS: `IOAVService`, the private door to the display's I²C bus.
//!
//! There is no public way to reach a monitor's control channel on macOS.
//! `IOAVService` is what MonitorControl and BetterDisplay use, it is not in
//! any header, and its three functions are resolved from IOKit at runtime the
//! same way `roadie-inject` resolves its private ApplicationServices SPI.
//!
//! # What this covers, and what it does not
//!
//! Apple silicon only. On those machines each external display appears in the
//! IOKit registry as a `DCPAVServiceProxy` whose `Location` is `External`, and
//! an `IOAVService` created from it carries I²C straight to the panel.
//!
//! Intel Macs use a different mechanism entirely — `IOI2CSendRequest` against
//! an `IOFramebuffer` service — and it is **deliberately not implemented
//! here**. Writing a second, untestable path against an API this project has
//! no machine for would produce code that looks like support and might be
//! nothing of the kind. An Intel Mac is told plainly that this build cannot
//! reach its monitors, which is worse for that user and honest about it.
//!
//! Neither path works on the built-in display of any Mac. That panel has no
//! DDC channel at all; its brightness is a system API, not a monitor feature.
//!
//! # The buffer split that is easy to get wrong
//!
//! `IOAVServiceWriteI2C` takes the chip address, then a *data address*, then a
//! buffer. The data address is not an extra field wrapped around the message:
//! it is the message's own first byte, the host address `0x51`, and the buffer
//! is everything after it. Passing the whole frame as the buffer sends `0x51`
//! twice and every monitor ignores the result.

use std::ffi::{CStr, c_char, c_int, c_void};
use std::num::NonZeroU32;
use std::ptr::NonNull;
use std::sync::OnceLock;

use objc2_core_foundation::{CFRetained, CFString, CFType};
use objc2_io_kit::{
    IOIteratorNext, IOObjectRelease, IORegistryEntryCreateCFProperty, IOServiceGetMatchingServices,
    IOServiceMatching, io_iterator_t, io_object_t, kIOMainPortDefault,
};
use roadie_ddc::Edid;
use roadie_ddc::edid::{BLOCK_LEN, EDID_I2C_ADDRESS};
use roadie_ddc::packet::{Frame, I2C_ADDRESS};

use crate::backend::{DdcTransport, DisplayError};
use crate::ddc::Ddc;
use crate::{Display, DisplayId, backend};

/// `kIOReturnSuccess`, which shares its value with `KERN_SUCCESS`.
const SUCCESS: i32 = 0;

/// The IOKit class each external display's audio-video service appears under
/// on Apple silicon.
const AV_SERVICE_CLASS: &CStr = c"DCPAVServiceProxy";

/// The registry property saying whether a service is an external display or
/// the built-in panel, and the value that means external.
const LOCATION_KEY: &str = "Location";
/// The only `Location` with a DDC channel behind it.
const EXTERNAL: &str = "External";

/// A reference to an IOKit object this process owns, released on drop.
///
/// IOKit spells "no object" as `0`, so [`NonZeroU32`] turns that sentinel into
/// `None` rather than a handle that would later be released as if it were one.
struct IoObject(NonZeroU32);

impl IoObject {
    /// Adopt a reference IOKit created for us, or `None` for its null object.
    fn adopt(raw: io_object_t) -> Option<Self> {
        NonZeroU32::new(raw).map(Self)
    }

    fn raw(&self) -> io_object_t {
        self.0.get()
    }
}

impl Drop for IoObject {
    fn drop(&mut self) {
        let _ = IOObjectRelease(self.0.get());
    }
}

/// An `IOAVService`, released on drop.
///
/// The type is private and has no binding, but it is an ordinary
/// CoreFoundation object underneath, so it is adopted as a [`CFRetained`] of
/// the opaque base type. That keeps the release where every other release in
/// this workspace is — in a `Drop` the framework crate wrote — rather than
/// hand-balanced here.
struct AvService(CFRetained<CFType>);

impl AvService {
    /// Adopt the +1 reference `IOAVServiceCreateWithService` returned.
    #[expect(
        unsafe_code,
        reason = "adopting a +1 CoreFoundation reference from a private SPI"
    )]
    fn adopt(raw: *mut c_void) -> Option<Self> {
        let raw = NonNull::new(raw)?.cast::<CFType>();
        // SAFETY: `IOAVServiceCreateWithService` follows the CoreFoundation
        // create rule, so the reference it returned is owned by this process
        // and is adopted here exactly once.
        Some(Self(unsafe { CFRetained::from_raw(raw) }))
    }

    /// The handle, in the shape the private calls want.
    fn raw(&self) -> *mut c_void {
        std::ptr::from_ref(&*self.0).cast_mut().cast::<c_void>()
    }
}

/// The three private IOKit symbols this backend needs.
///
/// `IOAVServiceCreateWithService` takes a CoreFoundation allocator and an
/// IOKit service and returns a +1 reference. The two transfer calls take the
/// chip address, a data address, a buffer and its length, and return an
/// `IOReturn`.
type CreateWithService = unsafe extern "C" fn(*const c_void, io_object_t) -> *mut c_void;
type WriteI2c = unsafe extern "C" fn(*mut c_void, u32, u32, *const c_void, u32) -> i32;
type ReadI2c = unsafe extern "C" fn(*mut c_void, u32, u32, *mut c_void, u32) -> i32;

/// The resolved symbols, or `None` on a system that does not have them.
struct Symbols {
    create: CreateWithService,
    write: WriteI2c,
    read: ReadI2c,
}

/// Resolve the three symbols once, or report that this machine has none.
///
/// A machine without them is an Intel Mac, or a macOS version that moved them.
/// Both are "this build cannot reach your monitors", not a crash.
#[expect(
    unsafe_code,
    reason = "IOAVService is private SPI with no header; dlopen and dlsym are the only way in"
)]
fn symbols() -> Option<&'static Symbols> {
    static SYMBOLS: OnceLock<Option<Symbols>> = OnceLock::new();
    SYMBOLS
        .get_or_init(|| {
            const RTLD_LAZY: c_int = 0x1;
            const IOKIT: &CStr = c"/System/Library/Frameworks/IOKit.framework/IOKit";

            // SAFETY: `dlopen` and `dlsym` come from libSystem, and both
            // arguments are NUL-terminated literals. The handle is
            // deliberately never closed: the symbols outlive every caller.
            let (create, write, read) = unsafe {
                let handle = dlopen(IOKIT.as_ptr(), RTLD_LAZY);
                if handle.is_null() {
                    return None;
                }
                (
                    dlsym(handle, c"IOAVServiceCreateWithService".as_ptr()),
                    dlsym(handle, c"IOAVServiceWriteI2C".as_ptr()),
                    dlsym(handle, c"IOAVServiceReadI2C".as_ptr()),
                )
            };
            if create.is_null() || write.is_null() || read.is_null() {
                return None;
            }
            // SAFETY: each pointer is a function `dlsym` resolved from IOKit,
            // and the signatures above are IOAVService's as MonitorControl and
            // BetterDisplay call them. A wrong signature here is undefined
            // behaviour, which is why they are declared once, in this file,
            // rather than at each call site.
            Some(unsafe {
                Symbols {
                    create: std::mem::transmute::<*mut c_void, CreateWithService>(create),
                    write: std::mem::transmute::<*mut c_void, WriteI2c>(write),
                    read: std::mem::transmute::<*mut c_void, ReadI2c>(read),
                }
            })
        })
        .as_ref()
}

#[expect(
    unsafe_code,
    reason = "dlopen and dlsym come from libSystem and have no safe binding"
)]
unsafe extern "C" {
    fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

/// One external display's I²C channel.
struct AvTransport {
    service: AvService,
    symbols: &'static Symbols,
    name: String,
}

impl AvTransport {
    /// Read `length` bytes from `chip`, which is the DDC address for a reply
    /// and the EEPROM address for the EDID.
    #[expect(unsafe_code, reason = "IOAVServiceReadI2C is private SPI")]
    fn read_from(&mut self, chip: u32, offset: u32, buffer: &mut [u8]) -> Result<(), DisplayError> {
        let length = u32::try_from(buffer.len()).unwrap_or(u32::MAX);
        // SAFETY: `service` is live for the lifetime of `self`, and the buffer
        // has at least `length` bytes — `length` is derived from its own len.
        let result = unsafe {
            (self.symbols.read)(
                self.service.raw(),
                chip,
                offset,
                buffer.as_mut_ptr().cast::<c_void>(),
                length,
            )
        };
        if result == SUCCESS {
            return Ok(());
        }
        Err(DisplayError::Transport {
            name: self.name.clone(),
            reason: format!("IOAVServiceReadI2C returned {result:#010x}"),
            // A display that is asleep or switching input refuses the transfer
            // for a moment. There is no errno to distinguish that from a
            // permanent refusal, so the retry budget above decides how long to
            // keep believing in it.
            retryable: true,
        })
    }

    /// The display's identification block, read off the EEPROM on the same bus.
    ///
    /// There is no public EDID on Apple silicon, and the private display
    /// services that carry one are a second undocumented dependency. The
    /// EEPROM is already reachable through the transport that exists, at the
    /// address the standard reserves for it, so this needs nothing new.
    fn edid(&mut self) -> Option<Edid> {
        let mut block = [0_u8; BLOCK_LEN];
        self.read_from(u32::from(EDID_I2C_ADDRESS), 0, &mut block)
            .ok()?;
        Edid::parse(&block).ok()
    }
}

impl DdcTransport for AvTransport {
    fn name(&self) -> String {
        self.name.clone()
    }

    #[expect(unsafe_code, reason = "IOAVServiceWriteI2C is private SPI")]
    fn send(&mut self, frame: &Frame) -> Result<(), DisplayError> {
        let bytes = frame.as_bytes();
        // The first byte is the data address and the rest is the buffer; see
        // this module's documentation. A frame is never empty, but splitting
        // it this way rather than indexing keeps that a fact rather than an
        // assumption.
        let Some((&address, payload)) = bytes.split_first() else {
            return Ok(());
        };
        let length = u32::try_from(payload.len()).unwrap_or(u32::MAX);
        // SAFETY: `service` is live for the lifetime of `self`, and `payload`
        // is a slice of the frame's own storage with `length` bytes.
        let result = unsafe {
            (self.symbols.write)(
                self.service.raw(),
                u32::from(I2C_ADDRESS),
                u32::from(address),
                payload.as_ptr().cast::<c_void>(),
                length,
            )
        };
        if result == SUCCESS {
            return Ok(());
        }
        Err(DisplayError::Transport {
            name: self.name.clone(),
            reason: format!("IOAVServiceWriteI2C returned {result:#010x}"),
            retryable: true,
        })
    }

    fn receive(&mut self, buffer: &mut [u8]) -> Result<usize, DisplayError> {
        self.read_from(u32::from(I2C_ADDRESS), 0, buffer)?;
        Ok(buffer.len())
    }
}

/// Every external display this Mac can reach over DDC.
pub(crate) fn enumerate() -> Result<Vec<Display>, DisplayError> {
    let Some(symbols) = symbols() else {
        return Err(DisplayError::Access {
            path: "IOKit".to_owned(),
            reason: "this Mac does not have the IOAVService interface. Monitor control \
                 here needs Apple silicon; Intel Macs use a different mechanism that this \
                 build does not yet implement"
                .to_owned(),
        });
    };

    // SAFETY: a NUL-terminated literal names the class, and the +1 dictionary
    // the call creates is adopted immediately.
    #[expect(unsafe_code, reason = "IOServiceMatching takes a C string")]
    let matching = unsafe { IOServiceMatching(AV_SERVICE_CLASS.as_ptr()) }.ok_or_else(|| {
        DisplayError::Access {
            path: "IOKit".to_owned(),
            reason: "IOServiceMatching could not describe the display service class".to_owned(),
        }
    })?;
    // The lookup consumes one reference to the dictionary, which this binding
    // spells by taking it by value, so handing it over is its last use.
    let matching = CFRetained::<objc2_core_foundation::CFDictionary>::from(&*matching);

    let mut raw: io_iterator_t = 0;
    // SAFETY: `kIOMainPortDefault` is IOKit's own static and `raw` is a live
    // local the call fills in on success.
    #[expect(unsafe_code, reason = "IOServiceGetMatchingServices is a C API")]
    let result =
        unsafe { IOServiceGetMatchingServices(kIOMainPortDefault, Some(matching), &raw mut raw) };
    if result != SUCCESS {
        return Err(DisplayError::Access {
            path: "IOKit".to_owned(),
            reason: format!("IOServiceGetMatchingServices returned {result:#010x}"),
        });
    }
    let Some(iterator) = IoObject::adopt(raw) else {
        return Ok(Vec::new());
    };

    let mut displays = Vec::new();
    while let Some(service) = IoObject::adopt(IOIteratorNext(iterator.raw())) {
        if !is_external(&service) {
            continue;
        }
        // SAFETY: `service` holds a live reference for the whole call, and the
        // allocator argument is CoreFoundation's default, spelled as null.
        #[expect(unsafe_code, reason = "IOAVServiceCreateWithService is private SPI")]
        let handle = unsafe { (symbols.create)(std::ptr::null(), service.raw()) };

        let index = displays.len() + 1;
        let Some(service) = AvService::adopt(handle) else {
            continue;
        };
        let mut transport = AvTransport {
            service,
            symbols,
            name: format!("display {index}"),
        };
        // Named from the panel's own EEPROM before anything else is asked of
        // it, so that every later error message can say which screen it is
        // about rather than which position it held in a list.
        let edid = transport.edid();
        if let Some(edid) = edid.as_ref() {
            transport.name = edid.describe();
        }
        displays.push(Display::new(
            DisplayId::new(format!("av-{index}")),
            edid,
            backend::boxed(Ddc::new(transport)),
        ));
    }
    Ok(displays)
}

/// Whether this service is an external display rather than the built-in panel.
///
/// The built-in panel has no DDC channel at all — its brightness is a system
/// API — so including it would put an entry in the list that can only ever
/// fail.
fn is_external(service: &IoObject) -> bool {
    let key = CFString::from_str(LOCATION_KEY);
    // SAFETY: `service` holds a live reference for the whole call, and `key` is
    // a live CFString, which IOKit dereferences without a null check.
    #[expect(unsafe_code, reason = "IORegistryEntryCreateCFProperty is a C API")]
    let property: Option<CFRetained<CFType>> =
        unsafe { IORegistryEntryCreateCFProperty(service.raw(), Some(&key), None, 0) };
    property
        .and_then(|value| value.downcast::<CFString>().ok())
        .is_some_and(|location| location.to_string() == EXTERNAL)
}
