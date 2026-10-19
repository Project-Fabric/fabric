# Benchmarks

Reproduce with:

```sh
cargo run --release -p fabric-snapshot --example capture_bench -- [files] [bytes-per-file]
```

The benchmark builds a synthetic project (default: 10,000 files of ~4 KB in 110 directories,
all dated an hour ago, as files in a real repository usually are) and times three captures,
asserting all three produce the same snapshot.

## Results

Windows 11, NTFS, Windows Defender real-time protection on, release build, October 2026.

| 10,000 files | Initial | + read before writing | + parallel (8 threads) |
|---|---|---|---|
| First capture (`fabric init`) | 52.5 s | 43.6 s | **21.1 s** |
| Recapture reading every file | 38.1 s | 1.47 s | **0.46 s** |
| Recapture with the stat cache (each agent step) | 0.65 s | 0.58 s | **0.24 s** |

1,000 files with the stat cache: 80 ms before parallelism.

## What the numbers mean

- **Agent steps use the stat cache.** A checkpoint after an agent step reads only files whose
  size, modification time or (on Unix) inode/change time differ from the last capture, plus
  files modified in the last 3 seconds (see `stat_cache.rs` for why). The remaining cost is
  walking the tree and one metadata call per file.
- **Content the store already has costs no writes.** Files up to 8 MB are read and hashed
  before anything is written, so unchanged content is never re-compressed or re-written.
- **The first capture writes every file durably.** Each new object is fsynced before it is
  renamed into place. Measured without the per-object fsync, the first capture took 8.3 s
  instead of 21.1 s; we keep it, because skipping it would let a power loss leave a
  checkpoint referring to content that never reached the disk. Most of the remaining time on
  this machine is file creation and antivirus scanning of each new file.

## Not yet measured

macOS and Linux (expected to be faster; NTFS metadata operations and Defender scanning
dominate on Windows), very large single files, and repositories over 100,000 files.
