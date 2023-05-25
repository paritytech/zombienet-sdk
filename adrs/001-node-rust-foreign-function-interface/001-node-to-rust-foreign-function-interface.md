# Mechanism to call Rust code from Javascript/Typescript

### Status: proposed | rejected | **accepted** | deprecated

### Deciders: [@pepoviola](https://github.com/pepoviola) [@wirednkod](https://github.com/wirednkod) [@l0r1s](https://github.com/l0r1s)

### Creation date: 18/05/2023

### Update date: -

---

## Context and Problem Statement

The `zombienet-sdk` will be developed in Rust. Our objective is make it easily integrable into existing Typescript/Javascript project. To achieve this goal, we need to find a way to call the Rust code from a Javascript/Typescript program.

Many mechanisms exists for this purpose like Wasm or N(ode)-API but some may or may not fit our use case, for example, executing async code.

---

## Decision drivers

- We can use the standard library (for filesystem or networking in providers).

- We can execute asynchronous code: our goal is not to make the program fully sequential as many operations (e.g: bootstrapping the relaychain nodes) can be done concurrently.

- Easy to package and deploy

---

## Considered Options

- #### WASM

  - [wasm-pack](https://github.com/neon-bindings/neon)

- #### Native node modules (Node-API / V8 / libuv)
  - [napi-rs](https://github.com/napi-rs/napi-rs)

---

## Prototyping

To demonstrate and learn which options fit the best for our use case, we will create a small test program which will have the following functionalities:

- Has a function taking an arbitratry object and a callback as parameters in the Typescript code, calling the callback with the function result on Rust side.
- Has a function taking an arbitrary object as parameter and a returning a promise in Typescript, signaling an asynchronous operation on Rust side.
- Make an HTTP request asynchronously in the Rust code, using a dependency using the standard library.

The prototype assume versions of `rustc` and `cargo` to be `1.69.0`, use of `stable` channel and `Linux` on `amd64` architecture.


- ### [Boilerplate app to execute prototype](boilerplate-app-prototype.md)

- ### [Wasm-pack prototype](wasm-prototype.md)

- ### [Napi-rs prototype](napi-prototype.md)

---

## Pros and cons of each options

- ### Napi-rs
  - Pros 👍
    - Support many types correctly including typed callback, typed array, class and all JS primitives types (Null, Undefined, Numbers, String, BigInt, ...)

    - Support top level async function because it detects if it needs to be run inside an async runtime (tokio by default)

    - Standard library can be used without limitations, including threading, networking, etc...

    - Extremely well documented with examples

    - Provide full Github action pipeline template to compile on all architecture easily

    - Support complex use cases

    - Used by many big names (Prisma, Parcel, Tailwind, Next.js, Bitwarden)

  - Cons 👎
    - Node-API is not simple for complex use case

    - Bound to NodeJS, if we want to expose the same logic to others languages (Go, C++, Python, ...) we need to wrap the Rust code inside a dynamic library and adapt to others languages primitives by creating a small adapter over the library

    - Not universally compiled


- ### Wasm-pack
  - Pros 👍
    - Rich ecosystem and developing fast

    - Used in many places across web, backend (Docker supports WASM)

    - Easy to use and distribute

    - Universally compiled and used across languages (if they support WASM execution)

    - Good for simple use case where you do pure function (taking input, returning output, without side effects like writing to filesystem or making networking calls)

  - Cons 👎
    - Limited in the use of the standard library, can't access networking/filesystem primitives without having to use WASI which is inconsistent across languages/runtimes

    - Only support 32 bits

    - No support for concurrent programming (async/threads), even if we can returns Promise from WASM exposed functions but could see the light in few months (maybe?)

    - wasm-bindgen types are too generic, for example, we return a JsValue but we would like to be more specific for the type

## Decision outcome

- ### **Napi-rs** for crates dependant on async, filesystem or networking: *support*, *orchestrator*, *test-runner*, *providers* from [schema](https://github.com/paritytech/zombienet-sdk/issues/22)

- ### **Wasm-pack** for the rest of the crates: *configuration* from [schema](https://github.com/paritytech/zombienet-sdk/issues/22)