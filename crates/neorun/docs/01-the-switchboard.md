---
origin: generated
date: 2025-12-13
---

### The Physics of the Airlock

We are accustomed to thinking of the Runtime as a container—a box that holds memory and executes instructions. But in `isorun`, the Runtime is not a box; it is a switchboard. It is a routing layer that sits between the abstract intent of a WebAssembly component and the concrete reality of where that computation actually happens.

To understand the architecture you have constructed, we must trace the lifecycle of execution from the moment the universe is created to the moment a bit crosses the wire.

#### I. Genesis: The Static Ledger

The lifecycle begins before a single instruction executes. This is the **Linker Phase**.

In a standard Wasmtime embedder, the Linker is a dictionary. You map string keys (`wasi:filesystem/types`) to function pointers. But in `neorun`, the Linker is a negotiator.

When you call `InstanceBuilder::link_remote`, you are performing an act of deception. The Guest Component expects an import—a function that it can call synchronously, passing values on the stack or in registers. It expects a neighbour. You, however, are providing a diplomat.

This is where the **Introspection** logic you wrote becomes critical. You cannot simply forward bytes blind. You must consult the **WIT Ledger**.
1.  You inspect the Component’s type section.
2.  You extract the schema: "This import `kv.get` takes a `String` and returns a `Result<Option<String>, Error>`."
3.  You bake this schema into a closure—a "Dynamic Stub"—and hand that stub to the Wasmtime Linker.

The Linker accepts this stub. As far as the Linker is concerned, the dependency is satisfied. The trap is set.

#### II. The Local Call: The Hydraulic Press

To understand the cost of the remote call, we must first look at the control group: the **Local Call**.

When a Guest calls a local `Linkable::System` function (like your `InMemoryKv`), the physics are those of a hydraulic press.
1.  The Guest pushes arguments onto the stack or writes them to linear memory.
2.  The CPU jumps.
3.  Wasmtime’s trampoline code reads the stack/memory directly.
4.  It constructs Rust types. If the Guest passes a `u32`, it is a register move. If it passes a string, it is a pointer/len pair and a UTF-8 check.
5.  Your Host implementation runs.
6.  The result flows back down the same path.

This is fast. It is synchronous. It shares the same address space (process memory). The friction is negligible because the abstraction layer (WIT) maps almost 1:1 to the machine layer (Assembly).

#### III. The Remote Call: The Airlock Protocol

Now, consider the **Remote Call**. The Guest executes the same instruction: `call $import_index`. But this time, it hits your Dynamic Stub.

Execution does not flow; it halts. We enter the **Airlock**.

**1. The Lifting (Materialization)**
Wasmtime cannot pass raw stack pointers to a remote machine. It must materialize the abstract values. The engine looks at the canonical ABI, reads the Guest memory, and constructs a `Vec<Val>`.
This is the first cost. We are converting a compact binary representation in linear memory into a heap-allocated Abstract Syntax Tree (`Val::String`, `Val::Record`). We have left the realm of the machine and entered the realm of the interpreter.

**2. The Encoding (The Neopack State Machine)**
Now the `neorpc` logic takes over. We are holding a `Val` in one hand and the **WIT Ledger** (the Type) in the other. We must serialize.
We initialize the `neopack::Encoder`. It is a strict state machine ("TigerStyle").
*   We see a `Val::Record`. We tell the Encoder `map_begin()`.
*   The Encoder checks its stack: "Am I allowed to start a map here?" Yes. It writes the Tag. It reserves 4 bytes for the length.
*   We iterate the fields. We verify against the Ledger that the fields match the schema.
*   We write the scalars. `enc.u32()`, `enc.str()`. These are raw writes to a `Vec<u8>`.
*   We call `map_end()`. The Encoder back-patches the length.

This process transforms the AST back into a linear format, but unlike the Guest's linear memory, this format is **self-describing** (TLV) and **bounded**. It is safe to transmit.

**3. The Void (Transport)**
The `CallFrame` is finalized. We have a buffer of bytes. We hand this to the `Transport` trait.
Here, the stack separates.
The Guest is suspended. In an async runtime (which you have correctly enabled), the Wasmtime "fiber" yields. The OS thread is free to do other work. The bytes travel over TCP, QUIC, or a channel. They are gone.

**4. The Return (Resurrection)**
Time passes. A `ReplyFrame` arrives. The Transport wakes the runtime.
We initialize a `neopack::Decoder`. It is a zero-copy view over the network buffer.
Now we must do the inverse of step 2, but with higher stakes. We cannot trust the remote peer.
*   We consult the Ledger: "I am expecting a `Result<Option<String>>`."
*   We ask the Decoder: `dec.result()?`.
*   The Decoder checks the tag. Is it `Tag::ResultOk` or `Tag::ResultErr`? If it is `Tag::U32`, it fails immediately.
*   We descend. `ok_dec.option()?`. `some_dec.str()?`.
*   We reconstruct the `Val`.

**5. The Lowering (Re-entry)**
We hold the result `Val`. We return it to Wasmtime.
Wasmtime takes this AST and "lowers" it. It allocates space in the Guest's linear memory (calling `cabi_realloc` if necessary), writes the UTF-8 bytes, puts the pointer/len on the stack, and resumes the Guest.

The Guest wakes up. It has no idea that its data just traveled to a different continent and back.

### The Architect's Dilemma

You have successfully threaded the needle. You have built a system where:
1.  **Safety** is preserved by the `neopack` state machine and the `neorpc` type verification.
2.  **Abstraction** is preserved by the Component Model; the Guest is agnostic to the implementation.
3.  **Concurrency** is preserved by the async runtime and the `Transport` boundary.

The cost is **Double Marshalling**.
*   Guest Memory -> `Val` AST (Wasmtime)
*   `Val` AST -> `neopack` Bytes (You)

In a purely local system, this would be unacceptable overhead. In a distributed system, this overhead is the price of admission. It is the cost of decoupling the memory space of the caller from the memory space of the callee.

You have built the Airlock. It is airtight. It is safe. Now you can let the bits flow.
