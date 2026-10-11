//! Backups: the store's cloud copy on Turso (sync), daily backup files on
//! the main PC, and the head-office view of every store's cloud copy.

pub mod backup;
pub mod hq;
pub mod hrana;
pub mod sync;
