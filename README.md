# Quart

Quart is a file format for the compression and archiving of multiple files, with per-file compression (and in future, encryption).

More information about Quart's workings can be found in the `src/compress.rs` and `src/extract.rs` files.

## Layout

For all values in Quart, if they are said to be represented by `varint`, then they will be stored as a minimum of one byte, this byte represents the size (in bytes) of the actual value, if the length is over 0, there will always be at least one other byte following it.

All of Quart uses little endian.

```ascii
┌─────────────────────┐
│  Archive Header     │  Small, just magic + version + flags
├─────────────────────┤
│  Compressed Data    │  ← File 1 data (no header, just bytes)
│  Compressed Data    │  ← File 2 data (squished directly against file 1)
│  Compressed Data    │  ← File 3 data
│  ...                │
├─────────────────────┤
│  Central Directory  │  ← All metadata + offsets here
│    - File Record 1  │     (name, offset, length, checksum, etc.)
│    - File Record 2  │
│    - File Record 3  │
│    - ...            │
├─────────────────────┤
│  End Record         │  ← Points back to Central Directory start
└─────────────────────┘
```

### Archive Header

┌────────────────────────────┐
│ Magic:   "QUART" (5 bytes) │ ← Always constant
│ Version: u16     (2 bytes) │ ← ID of the format version
└────────────────────────────┘

### Central Directory

┌──────────────────────────────────────────────────────────┐
│ Directory Entry 1                                        │
│ ┌──────────────────────────────────────────────────────┐ │
│ │ Filename Length:      varint (e.g., 15)              │ │
│ │ Filename:             UTF-8 bytes ("document.txt")   │ │
│ │ Data Offset:          varint (from archive start)    │ │
│ │ Compressed Size:      varint (bytes)                 │ │
│ │ Uncompressed Size:    varint (bytes)                 │ │
│ │ SHA-256 Hash:         32 bytes                       │ │
│ │ Modified Time:        u64 (Unix timestamp)           │ │
│ │ File Permissions:     u32 (Unix-style)               │ │
│ │ Compression Algo:     u8 (0=none, 1=zstd, 2=lzma...) │ │
│ │ Compression Level:    u8 (1-22 for zstd, etc.)       │ │
│ │ Encryption Algo:      u8 (0=none, 1=AES-256...)      │ │
│ │ Encryption Key ID:    u16 (0=none, else key group)   │ │
│ │ Attributes Length:    varint                         │ │
│ │ Attributes Data:      variable bytes (optional)      │ │
│ └──────────────────────────────────────────────────────┘ │
├──────────────────────────────────────────────────────────┤
│ Directory Entry 2                                        │
│ ┌──────────────────────────────────────────────────────┐ │
│ │ Filename Length:      varint                         │ │
│ │ Filename:             UTF-8 bytes                    │ │
│ │ Data Offset:          varint                         │ │
│ │ ... (same structure as entry 1)                      │ │
│ └──────────────────────────────────────────────────────┘ │
├──────────────────────────────────────────────────────────┤
│ ...                                                      │
└──────────────────────────────────────────────────────────┘

### End Record

┌──────────────────────────────────────────────┐
│ Central Dir Offset:      u64 (byte offset)   │
│ Central Dir Comp Size:   u32 (compressed)    │
│ Central Dir Uncomp Size: u32 (decompressed)  │
│ Entry Count:             u32 (# of files)    │
│ Compression Algo:        u8  (for directory) │
└──────────────────────────────────────────────┘

## License

Copyright (C) 2025 Jack Bacon

This program is free software: you can redistribute it and/or modify it under
the terms of the GNU Affero General Public License as published by the Free
Software Foundation, either version 3 of the License, or (at your option)
any later version.

This program is distributed in the hope that it will be useful, but WITHOUT
ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS
FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License for more
details.

You should have received a copy of the GNU Affero General Public License along
with this program. If not, see <https://www.gnu.org/licenses/>.
