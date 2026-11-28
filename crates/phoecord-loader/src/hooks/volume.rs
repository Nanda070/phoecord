//! VolumeBooster's patch (task 2.3.16): the instructions that use the volume ceiling are pointed
//! at a value of PhoeCord's own ([`crate::ui::voice`] says why not the constant itself).
//!
//! Only the four displacement bytes of each instruction change, after checking that the
//! instruction is the expected one and that it reads exactly the client's `200.0`. The new value
//! lives in one page allocated within reach of the code, kept for the life of the process.
//! Places that only work together (a slider's drag, fill and keys) are patched all or none
//! ([`lift_all`]).

use crate::ui::voice::{self, INSTRUCTION_LEN, NATIVE_CEILING};
use std::ffi::c_void;
use std::fmt;
use std::ops::Range;
use windows_sys::Win32::System::Diagnostics::Debug::FlushInstructionCache;
use windows_sys::Win32::System::Memory::{
    MEM_COMMIT, MEM_FREE, MEM_RELEASE, MEM_RESERVE, MEMORY_BASIC_INFORMATION,
    PAGE_EXECUTE_READWRITE, PAGE_READONLY, PAGE_READWRITE, VirtualAlloc, VirtualFree,
    VirtualProtect, VirtualQuery,
};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

#[derive(Debug, PartialEq, Eq)]
pub enum PatchError {
    /// The address is not inside the image.
    OutOfImage,
    /// Not the expected instruction.
    UnexpectedCode,
    /// The instruction does not load the client's ceiling from inside the image.
    UnexpectedCeiling,
    /// No free memory within reach of the instruction.
    NoMemoryNear,
    /// Windows refused to change the page protection.
    Protect,
}

impl fmt::Display for PatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            PatchError::OutOfImage => "the address is outside the image",
            PatchError::UnexpectedCode => "the code there is not the expected instruction",
            PatchError::UnexpectedCeiling => "the load there is not of the 200 % ceiling",
            PatchError::NoMemoryNear => "no free memory near the code",
            PatchError::Protect => "the page protection could not be changed",
        })
    }
}

impl std::error::Error for PatchError {}

/// A read-only `f32` close enough to `near` for a `disp32` to reach it.
///
/// The page is never freed once a patch points at it.
#[derive(Debug)]
pub struct Slot(usize);

impl Slot {
    /// Allocates the page below `near` (the module's base or lower) and writes `value`.
    pub fn near(near: usize, value: f32) -> Result<Slot, PatchError> {
        // Allocation granularity of Windows; 1 GiB keeps every instruction of a smaller module
        // within the 2 GiB a `disp32` reaches.
        const STEP: usize = 0x1_0000;
        const REACH: usize = 0x4000_0000;
        let start = near & !(STEP - 1);
        for back in (STEP..REACH).step_by(STEP) {
            let Some(candidate) = start.checked_sub(back).filter(|&a| a != 0) else {
                break;
            };
            let mut info = MEMORY_BASIC_INFORMATION::default();
            // SAFETY: `VirtualQuery` only reads the address space map; `info` is a valid buffer
            // of the size given.
            let got = unsafe {
                VirtualQuery(
                    candidate as *const c_void,
                    &mut info,
                    size_of::<MEMORY_BASIC_INFORMATION>(),
                )
            };
            if got == 0 || info.State != MEM_FREE {
                continue;
            }
            // SAFETY: asks for one new page at a free address; fails instead of overlapping.
            let page = unsafe {
                VirtualAlloc(
                    candidate as *const c_void,
                    size_of::<f32>(),
                    MEM_COMMIT | MEM_RESERVE,
                    PAGE_READWRITE,
                )
            };
            if page.is_null() {
                continue;
            }
            // SAFETY: `page` is a fresh writable page, aligned for any type.
            unsafe { page.cast::<f32>().write(value) };
            let mut old = 0;
            // SAFETY: changes the protection of the page just allocated.
            if unsafe { VirtualProtect(page, size_of::<f32>(), PAGE_READONLY, &mut old) } == 0 {
                // SAFETY: nothing points at the page yet.
                unsafe { VirtualFree(page, 0, MEM_RELEASE) };
                return Err(PatchError::Protect);
            }
            return Ok(Slot(page as usize));
        }
        Err(PatchError::NoMemoryNear)
    }
}

/// One patched load; [`Lifted::restore`] puts the client's displacement back.
#[derive(Debug)]
pub struct Lifted {
    at: usize,
    original: [u8; 4],
}

/// Points the operand of the instruction at address `at`, which starts with `opcode`, to `slot`.
///
/// # Safety
/// `image` must be a module image mapped and readable in this process for its whole length, and
/// `at` the instruction located by its signature. No thread may be executing that instruction
/// while it is rewritten (the loader does this at client start, before any call).
pub unsafe fn lift(
    image: Range<usize>,
    at: usize,
    opcode: [u8; 4],
    slot: &Slot,
) -> Result<Lifted, PatchError> {
    let inside = |a: usize, len: usize| {
        a >= image.start && a.checked_add(len).is_some_and(|end| end <= image.end)
    };
    if !inside(at, INSTRUCTION_LEN) {
        return Err(PatchError::OutOfImage);
    }
    // SAFETY: the instruction lies inside the readable image (checked above).
    let code = unsafe { (at as *const [u8; INSTRUCTION_LEN]).read_unaligned() };
    if code[..4] != opcode {
        return Err(PatchError::UnexpectedCode);
    }
    let constant = voice::loaded_from(code, opcode, at)
        .filter(|&a| inside(a, size_of::<f32>()))
        .ok_or(PatchError::UnexpectedCeiling)?;
    // SAFETY: the constant lies inside the readable image (checked above).
    let value = unsafe { (constant as *const f32).read_unaligned() };
    if value.to_bits() != NATIVE_CEILING.to_bits() {
        return Err(PatchError::UnexpectedCeiling);
    }
    let new = voice::displacement(at, slot.0).ok_or(PatchError::NoMemoryNear)?;
    let original = [code[4], code[5], code[6], code[7]];
    // SAFETY: the four bytes are the instruction's displacement, inside the image; the caller
    // keeps other threads off the instruction.
    unsafe { write_code(at + 4, new)? };
    Ok(Lifted { at, original })
}

/// [`lift`] for places that only make sense together: if one fails, those already patched are
/// put back and nothing changes.
///
/// # Safety
/// [`lift`]'s contract for every `(address, opcode)`.
pub unsafe fn lift_all(
    image: Range<usize>,
    places: &[(usize, [u8; 4])],
    slot: &Slot,
) -> Result<(), PatchError> {
    let mut done = Vec::new();
    for &(at, opcode) in places {
        // SAFETY: forwarded from the caller's contract.
        match unsafe { lift(image.clone(), at, opcode, slot) } {
            Ok(lifted) => done.push(lifted),
            Err(e) => {
                for lifted in done {
                    // SAFETY: as above. A refused restore leaves that one lifted; the error of
                    // the patch is the one to report.
                    let _ = unsafe { lifted.restore() };
                }
                return Err(e);
            }
        }
    }
    Ok(())
}

impl Lifted {
    /// Puts the original displacement back.
    ///
    /// # Safety
    /// The image must still be mapped, and no thread may be executing the instruction.
    pub unsafe fn restore(self) -> Result<(), PatchError> {
        // SAFETY: the same four bytes [`lift`] changed; forwarded from the caller's contract.
        unsafe { write_code(self.at + 4, self.original) }
    }
}

/// Overwrites four bytes of code and restores the page protection.
///
/// # Safety
/// `at..at + 4` must be mapped code of this process that no thread is executing.
unsafe fn write_code(at: usize, bytes: [u8; 4]) -> Result<(), PatchError> {
    let target = at as *mut [u8; 4];
    let mut old = 0;
    // SAFETY: changes the protection of the pages holding the caller's four bytes.
    if unsafe { VirtualProtect(target.cast(), 4, PAGE_EXECUTE_READWRITE, &mut old) } == 0 {
        return Err(PatchError::Protect);
    }
    // SAFETY: the bytes are writable now; the caller owns the write.
    unsafe { target.write_unaligned(bytes) };
    let mut ignored = 0;
    // SAFETY: puts back the protection the pages had.
    unsafe { VirtualProtect(target.cast(), 4, old, &mut ignored) };
    // SAFETY: tells the processor that this process's code changed.
    unsafe { FlushInstructionCache(GetCurrentProcess(), target.cast(), 4) };
    Ok(())
}

#[cfg(all(test, target_arch = "x86_64"))]
mod tests {
    use super::*;
    use windows_sys::Win32::System::Memory::PAGE_EXECUTE_READ;

    /// `movss xmm6, [rip+disp32]`.
    const LOAD: [u8; 4] = [0xF3, 0x0F, 0x10, 0x35];

    /// A page holding a function that returns the `f32` it loads into `xmm6`, as the client's
    /// clamp does, and the constant it loads. Returns the page and the load's address.
    fn stand_in(constant: f32) -> (usize, usize) {
        #[rustfmt::skip]
        let mut code: Vec<u8> = vec![
            0x48, 0x83, 0xEC, 0x18,             // sub rsp, 0x18
            0x0F, 0x11, 0x34, 0x24,             // movups [rsp], xmm6
            0xF3, 0x0F, 0x10, 0x35, 0, 0, 0, 0, // movss xmm6, [rip+disp32]
            0x0F, 0x28, 0xC6,                   // movaps xmm0, xmm6
            0x0F, 0x10, 0x34, 0x24,             // movups xmm6, [rsp]
            0x48, 0x83, 0xC4, 0x18,             // add rsp, 0x18
            0xC3,                               // ret
        ];
        let (load, constant_at) = (8usize, 64usize);
        let disp = (constant_at - (load + INSTRUCTION_LEN)) as i32;
        code[load + 4..load + 8].copy_from_slice(&disp.to_le_bytes());
        code.resize(constant_at, 0xCC);
        code.extend_from_slice(&constant.to_le_bytes());
        // SAFETY: a fresh page, written within its size and then made executable.
        unsafe {
            let page = VirtualAlloc(
                std::ptr::null(),
                0x1000,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_READWRITE,
            );
            assert!(!page.is_null());
            std::ptr::copy_nonoverlapping(code.as_ptr(), page.cast::<u8>(), code.len());
            let mut old = 0;
            assert_ne!(VirtualProtect(page, 0x1000, PAGE_EXECUTE_READ, &mut old), 0);
            (page as usize, page as usize + load)
        }
    }

    fn call(page: usize) -> f32 {
        // SAFETY: `page` starts with the stand-in function built above.
        let f: extern "C" fn() -> f32 = unsafe { std::mem::transmute(page as *const c_void) };
        f()
    }

    #[test]
    fn the_ceiling_is_lifted_and_restored() {
        let (page, load) = stand_in(NATIVE_CEILING);
        assert_eq!(call(page), 200.0);
        let slot = Slot::near(page, 1000.0).unwrap();
        // SAFETY: the stand-in page is the image; only this thread calls it.
        let lifted = unsafe { lift(page..page + 0x1000, load, LOAD, &slot) }.unwrap();
        assert_eq!(call(page), 1000.0);
        // A second patch finds another value than the client's and leaves it.
        // SAFETY: as above.
        let again = unsafe { lift(page..page + 0x1000, load, LOAD, &slot) };
        assert_eq!(again.unwrap_err(), PatchError::UnexpectedCeiling);
        // SAFETY: as above.
        unsafe { lifted.restore() }.unwrap();
        assert_eq!(call(page), 200.0);
    }

    #[test]
    fn anything_but_the_expected_load_is_left_alone() {
        let (page, load) = stand_in(100.0);
        let slot = Slot::near(page, 1000.0).unwrap();
        let image = page..page + 0x1000;
        // SAFETY: the stand-in page is the image; nothing is rewritten in these cases.
        unsafe {
            let other_value = lift(image.clone(), load, LOAD, &slot);
            assert_eq!(other_value.unwrap_err(), PatchError::UnexpectedCeiling);
            let other_code = lift(image.clone(), load + 1, LOAD, &slot);
            assert_eq!(other_code.unwrap_err(), PatchError::UnexpectedCode);
            let outside = lift(image, page + 0x1000 - 4, LOAD, &slot);
            assert_eq!(outside.unwrap_err(), PatchError::OutOfImage);
        }
        assert_eq!(call(page), 100.0);
    }

    #[test]
    fn a_group_is_lifted_whole_or_not_at_all() {
        let (page, load) = stand_in(NATIVE_CEILING);
        let slot = Slot::near(page, 1000.0).unwrap();
        let image = page..page + 0x1000;
        // SAFETY: the stand-in page is the image; only this thread calls it.
        let failed = unsafe { lift_all(image.clone(), &[(load, LOAD), (load + 1, LOAD)], &slot) };
        assert_eq!(failed, Err(PatchError::UnexpectedCode));
        assert_eq!(call(page), 200.0);
        // SAFETY: as above.
        unsafe { lift_all(image, &[(load, LOAD)], &slot) }.unwrap();
        assert_eq!(call(page), 1000.0);
    }
}
