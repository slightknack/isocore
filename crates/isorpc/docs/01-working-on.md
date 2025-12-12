I am working on a translation layer for WIT (WebAssembly Interface Types). I want you to read about the canonical ABI. I am using wasmtime. The goal is to allow for rpc calls between different wasm components running on different machines, as long as they correspond to the same interface. Each rpc call has a:

- sequence number (starts at zero, grows with each call)
- a target wasm component instance, which is a string e.g. "kv-store"
- method name, which is a string e.g. "get"
- a list of arguments, which are Wasmtime Val.

Each rpc reply is either successful or fails. If it succeds we have:

- sequence number, same as the rpc call sequence number
- a list of return arguments, which are Wasmtime Val

If it fails we have:

- a sequence number, as before
- a reason for failure
  - the application crashed (trapped)
  - the application is out of compute (fuel)
  - the application is out of memory (passed set limits)
  - the wasm component instance has died or no longer exists
  - the wasm component instance does not supply a method of that type
  - the arguments are incorrect
  - and so on.

I would like you to do the following:

- read the appropriate documentation and design docs for wasmtime and wasm components
- write structs for rpc call, rpc success, rpc failure
- write an encoder from wasmtime Val to a neopack message, and a decoder from a neopack message to wasmtime Val. You do not need to handle resources, futures, streams, or error contexts. The decoder must take a Wamstime type so that it knows how to decode the neopack message. The same may be true of the encoder, though I believe Val carries enough information
- create functions to build rpc calls and replies. They must use the encoder to encode each argument in the call list or the return list.
- create functions to turn rpc messages into byte messages that can be sent on the wire. These functions take a rpc message and produce a byte sequence that can be written to the wire. messages should be skippable, i.e. have one root container that contains the tag and the length of the message, so multiple messages can be sent on the wire one after another. If you use neopack correctly, this behaviour should fall out naturally.

For now, return two files:

1. wasmrpc/lib.rs, which contains all types and documented function signatures
2. wasmrpc/tests.rs, which contains all

I suggest you first read and research all information you need, then draft the tests file with an idealized API, then draft lib.rs, iterate on lib.rs and tests.rs until everything looks good, revise the documentation, all while reasoning!

Once done thinking, output only lib.rs and tests.rs in markdown code blocks, along with any design decisions made.
