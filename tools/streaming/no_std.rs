//! Compile the identical grammar with neither std nor an allocator available.
//! This gate covers the new streaming module, not the legacy reader.
#![no_std]
#![forbid(unsafe_code)]

#[path = "../../src/crc.rs"]
pub mod crc;
#[path = "../../src/streaming/mod.rs"]
pub mod streaming;

const _: () = assert!(core::mem::size_of::<streaming::FlatHeaderParser>() <= 512);
const _: () = assert!(core::mem::size_of::<streaming::Checkpoint>() <= 272);
const _: () = assert!(core::mem::size_of::<streaming::FlatWalker>() <= 512);
const _: () = assert!(core::mem::size_of::<streaming::WalkCheckpoint>() <= 272);
const _: () = assert!(core::mem::size_of::<streaming::Lz77Decoder>() <= streaming::WINDOW + 128);
const _: () = assert!(core::mem::size_of::<streaming::Lz77Checkpoint>() <= streaming::WINDOW + 128);
const _: () = assert!(core::mem::size_of::<streaming::R2004HeaderParser>() <= 512);
const _: () = assert!(core::mem::size_of::<streaming::R2004Checkpoint>() <= 336);
const _: () = assert!(core::mem::size_of::<streaming::R2004MapWalker>() <= streaming::WINDOW + 1024);
const _: () = assert!(core::mem::size_of::<streaming::R2004MapCheckpoint>() <= streaming::WINDOW + 1024);
