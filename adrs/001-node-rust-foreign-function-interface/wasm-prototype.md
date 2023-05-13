## [Back](001-node-to-rust-foreign-function-interface.md)

## Wasm-pack prototype
___

1. Install the wasm-pack CLI

```bash
curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh
```

2. Create a new wasm-pack project

```bash
wasm-pack new wasm-prototype
```

3. Install cargo dependencies
```bash
cargo add tokio --features full
cargo add reqwest --features blocking
cargo add wasm-bindgen-futures
cargo add js-sys
```

4. Copy the following code to `wasm-prototype/src/lib.rs`
```rust
mod utils;

use wasm_bindgen::prelude::*;

// When the `wee_alloc` feature is enabled, use `wee_alloc` as the global
// allocator.
#[cfg(feature = "wee_alloc")]
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[wasm_bindgen]
pub async fn fetch_promise() -> Result<String, JsError> {
    let body = reqwest::get("https://paritytech.github.io/zombienet/")
        .await
        .map_err(|_| JsError::new("Error while fetching page"))?
        .text()
        .await
        .map_err(|_| JsError::new("Error while extracting body"))?;

    Ok(body)
}

#[wasm_bindgen]
pub fn fetch_callback(callback: &js_sys::Function) -> Result<JsValue, JsValue> {
    let this = JsValue::null();

    let response = reqwest::blocking::get("https://paritytech.github.io/zombienet/");

    if response.is_err() {
        return callback.call2(
            &this,
            &JsError::new("Error while fetching page").into(),
            &JsValue::null(),
        );
    }

    let body = response.unwrap().text();

    if body.is_err() {
        return callback.call2(
            &this,
            &JsError::new("Error while extracting body").into(),
            &JsValue::null(),
        );
    }

    Ok(body.unwrap().into())
}
```

5. Build the project :
```bash
wasm-pack build -t nodejs
```

Error are shown, this is expected because WASM doesn't support networking primitives, 
as you can see, we removed the thread call from the fetch_callback function because ```JsValue```
is using *const u8 under the hood and it's not ```Send``` so can't be passed safely across thread:

```bash
[INFO]: 🎯  Checking for the Wasm target...
[INFO]: 🌀  Compiling to Wasm...
   Compiling mio v0.8.6
   Compiling parking_lot v0.12.1
   Compiling serde_json v1.0.96
   Compiling url v2.3.1
error[E0432]: unresolved import `crate::sys::IoSourceState`
  --> /home/user/.cargo/registry/src/github.com-1ecc6299db9ec823/mio-0.8.6/src/io_source.rs:12:5
   |
12 | use crate::sys::IoSourceState;
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^ no `IoSourceState` in `sys`

error[E0432]: unresolved import `crate::sys::tcp`
  --> /home/user/.cargo/registry/src/github.com-1ecc6299db9ec823/mio-0.8.6/src/net/tcp/listener.rs:15:17
   |
15 | use crate::sys::tcp::{bind, listen, new_for_addr};
   |                 ^^^ could not find `tcp` in `sys`

error[E0432]: unresolved import `crate::sys::tcp`
  --> /home/user/.cargo/registry/src/github.com-1ecc6299db9ec823/mio-0.8.6/src/net/tcp/stream.rs:13:17
   |
13 | use crate::sys::tcp::{connect, new_for_addr};
   |                 ^^^ could not find `tcp` in `sys`

error[E0433]: failed to resolve: could not find `Selector` in `sys`
   --> /home/user/.cargo/registry/src/github.com-1ecc6299db9ec823/mio-0.8.6/src/poll.rs:301:18
    |
301 |             sys::Selector::new().map(|selector| Poll {
    |                  ^^^^^^^^ could not find `Selector` in `sys`

error[E0433]: failed to resolve: could not find `event` in `sys`
  --> /home/user/.cargo/registry/src/github.com-1ecc6299db9ec823/mio-0.8.6/src/event/event.rs:24:14
   |
24 |         sys::event::token(&self.inner)
   |              ^^^^^ could not find `event` in `sys`

error[E0433]: failed to resolve: could not find `event` in `sys`
  --> /home/user/.cargo/registry/src/github.com-1ecc6299db9ec823/mio-0.8.6/src/event/event.rs:38:14
   |
38 |         sys::event::is_readable(&self.inner)
   |              ^^^^^ could not find `event` in `sys`

error[E0433]: failed to resolve: could not find `event` in `sys`
  --> /home/user/.cargo/registry/src/github.com-1ecc6299db9ec823/mio-0.8.6/src/event/event.rs:43:14
   |
43 |         sys::event::is_writable(&self.inner)
   |              ^^^^^ could not find `event` in `sys`

error[E0433]: failed to resolve: could not find `event` in `sys`
  --> /home/user/.cargo/registry/src/github.com-1ecc6299db9ec823/mio-0.8.6/src/event/event.rs:68:14
   |
68 |         sys::event::is_error(&self.inner)
   |              ^^^^^ could not find `event` in `sys`

error[E0433]: failed to resolve: could not find `event` in `sys`
  --> /home/user/.cargo/registry/src/github.com-1ecc6299db9ec823/mio-0.8.6/src/event/event.rs:99:14
   |
99 |         sys::event::is_read_closed(&self.inner)
   |              ^^^^^ could not find `event` in `sys`

error[E0433]: failed to resolve: could not find `event` in `sys`
   --> /home/user/.cargo/registry/src/github.com-1ecc6299db9ec823/mio-0.8.6/src/event/event.rs:129:14
    |
129 |         sys::event::is_write_closed(&self.inner)
    |              ^^^^^ could not find `event` in `sys`
```