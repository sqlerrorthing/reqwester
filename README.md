# reqwester

A small toolkit for building **type-safe API clients**: describe requests as payloads, an API as a trait, and get
composable wrappers (retries, tracing) and factories generated for you.

Inspired by [tower](https://github.com/tower-rs/tower) and
[teloxide](https://github.com/teloxide/teloxide).

## Overview

- **`Payload`**: the data of a request and its output type. `#[derive(Payload)]` also generates `new` and setters, and
  supports lifetimes.
- **`Request`**: a ready to send request (`send`, `send_ref`, or just `.await`).
- **`#[requester]`**: turns a trait of plain methods into request types and methods; works on the trait and on its
  impl. A method without a return type in an impl becomes `type X = impl Request<..>`.
- **Adapters**, requested in the attribute:
  - `factory`: `{Requester}Factory` that creates independent requesters.
  - `retryable`: requests are re-sent according to a chainable `RetryPolicy` that sees the error.
  - `traced`: a `tracing` span and an outcome event per request.
  - `erased`: a dyn-compatible `Dyn{Requester}` (implemented by `Erased{Requester}<R>`; `&`, `Box` and `Arc` of it are
    requesters), so differently wrapped requesters can live in one collection.

```rust
#[requester(Send, Clone, factory, retryable, traced, erased)]
pub trait Api {
    type Error: std::error::Error + Send + 'static;

    fn get_user<'a>(&self, id: &'a str);
}

let policy = Exponential::new(Duration::from_millis(100))
    .max_attempts(5)
    .when(|e: &Error| e.is_transient());

let api = api.traced().retryable(policy);
let user = api.get_user("42").await?;
```

## Crates and features

| Crate               | What                                                      |
|---------------------|-----------------------------------------------------------|
| `reqwester`         | The entry point, re-exports everything below              |
| `reqwester-core`    | `Payload`, `Request`                                      |
| `reqwester-macros`  | `#[derive(Payload)]`, `#[requester]`                      |
| `reqwester-util`    | Wrappers: `retryable`, `traced`, `erased` (`reqwester::util`) |

Features of `reqwester`: `macros` (the derives) and `util` (wrappers and the `retryable` / `traced` / `erased` adapters).

## Notes

- Writing `fn f(&self)` in an impl, without a return type, needs nightly
  (`#![feature(impl_trait_in_assoc_type)]`); with an explicit return type it works on stable.
- `retryable` and `erased` send clones of a request, so requests (and their payloads) have to be `Clone`;
  `erased` also needs `Send` and `Clone` on the trait.
