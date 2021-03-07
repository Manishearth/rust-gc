Changelog

# master

# 0.4.0

Major changes:
 - [Remove usage of specialization in finalizers. Finalizers must be manually implemented now.](https://github.com/Manishearth/rust-gc/pull/129)
 - [New defaulted type parameter added to `GcCellRefMut`](https://github.com/Manishearth/rust-gc/pull/123)

Bugfixes:
 - [`Gc::from_raw`: Set the `Gc` as a root](https://github.com/Manishearth/rust-gc/pull/122)
 - [`GcCellRefMut::drop`: Unroot the right value after `GcCellRefMut::map`](https://github.com/Manishearth/rust-gc/pull/123)
 - [`Gc::from_raw`: Rely only on documented guarantees to compute layout](https://github.com/Manishearth/rust-gc/pull/125)

API updates:
 - [Remove `T: Trace` bound from `GcCellRef<T>](https://github.com/Manishearth/rust-gc/pull/118)
 - [Add `GcCellRef::clone`](https://github.com/Manishearth/rust-gc/pull/118)
 - [Allow `#[derive(Trace)]` for unsized types](https://github.com/Manishearth/rust-gc/pull/112)
 - [Add `Trace` for `Rc`](https://github.com/Manishearth/rust-gc/pull/106)
 - [Fix `ptr_eq()` on rooted and unrooted references](https://github.com/Manishearth/rust-gc/pull/108)

Nightly Rust update fixes:
 - [Update to new name of `auto_traits` feature](https://github.com/Manishearth/rust-gc/pull/111)
 - [Fix deprecated trait objec syntax warnings](https://github.com/Manishearth/rust-gc/pull/119)
 - [Remove use of `auto_traits`](https://github.com/Manishearth/rust-gc/pull/127)