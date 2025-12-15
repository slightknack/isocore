---
origin: handwritten
date: 2025-12-14
---

Some feedback: 

- In bind.rs, I notice that we use to_vec to copy the vals. This is inefficient. What we should do is encode the vals, and then in the closure, get the sequence number, build the payload, e.g. Call should take a CallEncoder. If you want to make a method on client that increments seq and inserts pending, that would be great.

- I don't like this line:

```
// Skip WASI interfaces - they're system components, not RPC targets
if name.starts_with("wasi:") {
    continue;
}
```

We shouldn't have to treat WASI specially. Remove it. WASI is a perfectly valid RPC target in some cases, and may be implemented by other components, be it local or remote.

- for crates/exorun/src/mock_transport.rs, we can move this to tests.rs. I think a lot of the tests in tests.rs can be moved to either unit tests in individual files OR to integration tests in the exorun/tests folder.

- runtime looks mostly good, but like remove instance ideally should e.g. kill the app if it's running or something. This is something to look into

- There has to be a better way to implement bind_local_method
