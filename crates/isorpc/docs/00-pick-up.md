<!-- handwritten -->
<!-- 2025-12-12 -->

when we pick this up we need to:

- continue renaming encoder and decoder for consistency
- figure out what the right interface for RPC calls and get them working
  - essentially, we need to provide the type information of the function call and expected return
  - Calls have multiple arguments, and so can returns.
  - I don't know if we want to do this encoding here or in isopack.
    - I'm imagining, in isopack our arguments are some list of Vals
    - We use isorpc::encode to prepare an argument payload
    - We use isorpc::message (or something) to prepare the full message with the bytes we want to send over the wire
    - Then, on the other end, we can get the message, decode it, decode the arguments, call the wasm instance.
    - Really we only do this for actual RPC-type calls. For calls between components on the same machine, we do not want this overhead
    - Also, for sending the results: encode them manually, send the RPC message, decode it manually.
    - I want RPC to be more or less generic:
      - We have a stream of messages, each with a sequence number and tag for message type
      - To respond to a message, just reply with the sequence number
      - Each machine has their own space for sequence numbers.
- Once RPC works, it should be possible to:
  - spin up three runtimes locally, with common interfaces.
    - say machine 1 provides a kv service that writes to memory
    - then machine 2 can connect to machine 1 and write a value
    - then machine 3 can connect to machine 1 and read that same value
    - We should make a test that creates 3 runtimes and creates tcp connections between them (or something) and tries to do this. 
    - Maybe we'll need some sort of mock distributed testing harness. That sounds fun!
- I need to start thinking of services.
- e.g. auth, crypto, storage, serving static assets, webserver
