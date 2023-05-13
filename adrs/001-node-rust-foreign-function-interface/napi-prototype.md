## [Back](001-node-to-rust-foreign-function-interface.md)

## Napi-rs prototype
___

1. Install the napi CLI

```bash
npm install -g @napi-rs/cli
```

2. Create a new napi project

```bash
napi new napi-prototype
```

3. Install cargo dependencies

```bash
cargo add tokio --features full
cargo add reqwest --features blocking
cargo add napi --no-default-features --features napi4,async
```

4. Copy the following code to `napi-prototype/src/lib.rs`

```rust
#![deny(clippy::all)]

use std::thread;

use napi::{
  bindgen_prelude::*,
  threadsafe_function::{
    ErrorStrategy, ThreadSafeCallContext, ThreadsafeFunction, ThreadsafeFunctionCallMode,
  },
};
use reqwest;

#[macro_use]
extern crate napi_derive;

// native async with tokio is supported without annotating a main function
#[napi]
pub async fn fetch_promise() -> Result<String> {
  let body = reqwest::get("https://paritytech.github.io/zombienet/")
    .await
    .map_err(|_| napi::Error::from_reason("Error while fetching page"))?
    .text()
    .await
    .map_err(|_| napi::Error::from_reason("Error while extracting body"))?;

  Ok(body)
}

#[napi]
pub fn fetch_callback(callback: JsFunction) -> Result<()> {
  // createa thread safe callback from the JsFunction
  let thread_safe_callback: ThreadsafeFunction<String, ErrorStrategy::CalleeHandled> = callback
    .create_threadsafe_function(0, |ctx: ThreadSafeCallContext<String>| {
      ctx.env.create_string(&ctx.value).map(|s| vec![s])
    })?;

  // spawn a thread to execute our logic
  thread::spawn(move || {
    let response = reqwest::blocking::get("https://paritytech.github.io/zombienet/");

    if response.is_err() {
      let response = response
        .map(|_| "".into())
        .map_err(|_| napi::Error::from_reason("Error while fetching page"));

      // error are returned by calling the callback with an empty response and the error mapped
      return thread_safe_callback.call(response, ThreadsafeFunctionCallMode::Blocking);
    }

    let body = response.unwrap().text();

    if body.is_err() {
      let body = body
        .map(|_| "".into())
        .map_err(|_| napi::Error::from_reason("Error while extracting body"));

      return thread_safe_callback.call(body, ThreadsafeFunctionCallMode::Blocking);
    }

    // result is returned as a string
    thread_safe_callback.call(Ok(body.unwrap()), ThreadsafeFunctionCallMode::Blocking)
  });

  Ok(())
}
```

5. Build the project :
```bash
npm run build
```

6. Copy artifacts :
```
mv napi-prototype.linux-x64-gnu.node index.d.ts index.js npm/linux-x64-gnu
```

7. Install package in ```ffi-prototype/app``` :
```bash
npm i ../napi-prototype/npm/linux-x64-gnu/
```

8. Copy the following code to the ```ffi-prototype/app/index.ts``` file :

```ts
import { fetchCallback, fetchPromise } from "napi-prototype-linux-x64-gnu";

(async () => {
  fetchCallback((_err: any, result: string) => {
    console.log(`HTTP request through FFI with callback: ${result.length}`);
  });

  console.log(
    `HTTP request through FFI with promise ${(await fetchPromise()).length}`
  );
})();
```

9. Build and execute the app :

```bash
npm run build+exec
```

Expected output:
```tty
> app@1.0.0 build+exec
> tsc && node ./index.js

HTTP request through FFI with promise 12057
HTTP request through FFI with callback: 12057
```

That's it !