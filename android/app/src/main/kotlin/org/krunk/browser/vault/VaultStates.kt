package org.krunk.browser.vault

// The vault's states and results as the screens see them. `VaultController` and
// `BiometricUnlock` are written in parallel and declare the same names with the same members:
// when both are in one tree, this file goes and theirs stay.

enum class VaultState { NONE, LOCKED, UNLOCKED, KEY_LOST }

enum class LockReason { MANUAL, TIMER, BACKGROUND, PASSWORD_CHANGED, DESTROYED }

enum class BioAvailability { AVAILABLE, NO_HARDWARE, NONE_ENROLLED, UNSUPPORTED_OS, UNAVAILABLE }

enum class BioOffReason { ENROLLMENT_CHANGED, PASSWORD_CHANGED, BLOB_MISSING, KEY_MISSING }

enum class BioUnlockResult { UNLOCKED, CANCELLED, FELL_BACK, FAILED }
