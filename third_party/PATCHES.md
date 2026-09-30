# Vendored third-party code

| Library | Version | Source | License | Linked |
|---------|---------|--------|---------|--------|
| TagLib | 2.3.2 | https://github.com/taglib/taglib/releases/download/v2.3.2/taglib-2.3.2.tar.gz | LGPL-2.1 / MPL-1.1 (dual) | static, used under MPL-1.1 |
| utfcpp | bundled with TagLib 2.3.2 (`taglib/3rdparty/utfcpp`) | — | BSL-1.0 | header-only |

Removed from the TagLib tree to keep the repo small: `tests/`, `examples/`,
`doc/`, `3rdparty/utfcpp/tests`, `3rdparty/utfcpp/bench`.

## Local patches

### TagLib: `taglib/toolkit/tfilestream.cpp` — Windows share mode

Upstream opens every file with `FILE_SHARE_READ` only, even read-only. Two
TagLib handles on the same file therefore conflict as soon as one has write
access, so a tag write fails while a scan or artwork read has the file open.
The patch adds `FILE_SHARE_WRITE` to the `CreateFileW` call. It does not
change what TagLib reads or writes. Writes in the app are serialised
(`fl_tags::WRITE_LOCK`), so there is never more than one writer.
