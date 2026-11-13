//! A tiny game for the WASM memory inspector's checks: health (i32), score (f32), a struct
//! reached through a pointer stored in a static, and a memory that grows on request. Plain
//! `extern "C"` exports, no wasm-bindgen, so the page's loader is a few lines of JavaScript.
//!
//! Exported globals (the inspector's Instances list): `__stack_pointer`, mutable (build.rs asks
//! the linker to export it: the one mutable global a Rust module has); `VERSION`, an exported
//! static (in WebAssembly an immutable global that holds the static's address: the i32 there
//! is 19).
#![no_std]

use core::ptr::addr_of_mut;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

/// The player's health: what the check finds by scanning (exact 100, then decreased).
static mut HEALTH: i32 = 100;
/// The score.
static mut SCORE: f32 = 0.0;

/// Data whose address is stored in `PLAYER`: the pointer-path check reads `gold` as
/// [PLAYER] + 8.
#[repr(C)]
pub struct Player {
    pub level: i32,
    pub xp: i32,
    pub gold: i32,
}

static mut ARENA: [Player; 4] = [
    Player { level: 0, xp: 0, gold: 0 },
    Player { level: 0, xp: 0, gold: 0 },
    Player { level: 0, xp: 0, gold: 0 },
    Player { level: 0, xp: 0, gold: 0 },
];
static mut PLAYER: *mut Player = core::ptr::null_mut();

#[no_mangle]
pub extern "C" fn init() {
    // SAFETY: single-threaded wasm; the arena is static.
    unsafe {
        let p = addr_of_mut!(ARENA[2]);
        (*p).level = 3;
        (*p).xp = 120;
        (*p).gold = 250;
        PLAYER = p;
    }
}

#[no_mangle]
pub extern "C" fn hit(damage: i32) -> i32 {
    // SAFETY: single-threaded wasm.
    unsafe {
        HEALTH = (HEALTH - damage).max(0);
        HEALTH
    }
}

#[no_mangle]
pub extern "C" fn heal(amount: i32) -> i32 {
    // SAFETY: single-threaded wasm.
    unsafe {
        HEALTH += amount;
        HEALTH
    }
}

#[no_mangle]
pub extern "C" fn health() -> i32 {
    // SAFETY: single-threaded wasm.
    unsafe { HEALTH }
}

#[no_mangle]
pub extern "C" fn add_score(points: f32) -> f32 {
    // SAFETY: single-threaded wasm.
    unsafe {
        SCORE += points;
        SCORE
    }
}

#[no_mangle]
pub extern "C" fn score() -> f32 {
    // SAFETY: single-threaded wasm.
    unsafe { SCORE }
}

#[no_mangle]
pub extern "C" fn gold() -> i32 {
    // SAFETY: single-threaded wasm; PLAYER is set by init.
    unsafe {
        if PLAYER.is_null() {
            0
        } else {
            (*PLAYER).gold
        }
    }
}

#[no_mangle]
pub extern "C" fn spend(amount: i32) -> i32 {
    // SAFETY: single-threaded wasm; PLAYER is set by init.
    unsafe {
        if !PLAYER.is_null() {
            (*PLAYER).gold -= amount;
        }
    }
    gold()
}

/// An exported static: an immutable global whose value is this static's address.
#[no_mangle]
pub static VERSION: i32 = 19;

/// Where `grow` last put its marker (0: the memory has not grown).
static mut MARKER: *mut i32 = core::ptr::null_mut();

/// Grows the memory by one page (64 KiB) and writes 4242 sixteen bytes into the new page: an
/// address that did not exist before. Returns the memory's size in pages, or -1.
#[no_mangle]
pub extern "C" fn grow() -> i32 {
    let old = core::arch::wasm32::memory_grow(0, 1);
    if old == usize::MAX {
        return -1;
    }
    // SAFETY: single-threaded wasm; the page at `old * 65536` was just added.
    unsafe {
        let p = (old * 65536 + 16) as *mut i32;
        p.write(4242);
        MARKER = p;
    }
    pages()
}

/// The memory's size in pages.
#[no_mangle]
pub extern "C" fn pages() -> i32 {
    core::arch::wasm32::memory_size(0) as i32
}

/// The address of the marker in the newest page (0 before the first `grow`).
#[no_mangle]
pub extern "C" fn marker_addr() -> i32 {
    // SAFETY: single-threaded wasm.
    unsafe { MARKER as i32 }
}

/// The value at the marker (0 before the first `grow`): the page shows it, so a value written
/// there by the inspector shows up.
#[no_mangle]
pub extern "C" fn marker() -> i32 {
    // SAFETY: single-threaded wasm; MARKER is null or points into a grown page.
    unsafe {
        if MARKER.is_null() {
            0
        } else {
            MARKER.read()
        }
    }
}

/// Addresses for the checks' expected values only; the inspector finds everything by scanning.
#[no_mangle]
pub extern "C" fn player_ptr_addr() -> i32 {
    addr_of_mut!(PLAYER) as i32
}

#[no_mangle]
pub extern "C" fn health_addr() -> i32 {
    addr_of_mut!(HEALTH) as i32
}
