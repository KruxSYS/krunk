//! A tiny game for the WASM memory inspector's checks: health (i32), score (f32) and a struct
//! reached through a pointer stored in a static. Plain `extern "C"` exports, no wasm-bindgen,
//! so the page's loader is a few lines of JavaScript.
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

/// Addresses for the checks' expected values only; the inspector finds everything by scanning.
#[no_mangle]
pub extern "C" fn player_ptr_addr() -> i32 {
    addr_of_mut!(PLAYER) as i32
}

#[no_mangle]
pub extern "C" fn health_addr() -> i32 {
    addr_of_mut!(HEALTH) as i32
}
