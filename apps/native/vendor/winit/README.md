# winit 0.30.13

The library source and license come from the published winit 0.30.13 crate, built from upstream commit `e9809ef54b18499bb4f2cac945719ecc2a61061b`. The Cargo manifest omits the upstream examples, external tests, and packaging-only files. [Upstream source](https://github.com/rust-windowing/winit/tree/e9809ef54b18499bb4f2cac945719ecc2a61061b)

Android delivers a new NativeActivity and Looper when the activity is recreated within the same process. This fork exits the old loop on `MainEvent::Destroy` and releases winit’s event-loop creation guard when the Android event loop drops. The guard still rejects concurrent event loops. The changes are in `src/event_loop.rs` and `src/platform_impl/android/mod.rs`.
