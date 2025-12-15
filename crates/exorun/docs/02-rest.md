---
origin: handwritten
date: 2025-12-14
---

Instead of Arc<Mutex<Hashmap<...>>>, one thing to include is to use a DashMap instead.

Do not mention isorun by name, as that will be confusing for the implementor.

Be more detailed about how to go about the changes. Also include guidelines about coding style, like to write rust like "c with safety" or "as a go programmer would", while still being concise and using functional idioms. Also to prefer keeping implementations flat where possible and eschewing nested indentation, e.g. breaking out control flow into separate functions or using let-else over if-let in loops to keep the code flat. Make a comprehensive list of good style notes, borrowing from tigerstyle, etc.

Another thing is that if a file has errors it should include an Error enum, and if referring to another module's Error enum, it should use crate::module and then write module::Error. Instruct the implementor to make this change across all files, and to not use as ModuleError in imports. Be specific; you can point out all the places that need to change.

Specify that tests should test things at the boundary, including that things that work work correctly, and that things that fail fail in the expected way.

I would like to do distributed resources, but they need to be authenticated. I want to implement two wasm apps; one called auth that includes public-key cryptography for signatures etc, another called meta that essentially exposes a WIT interface to exorun backed by auth. Until I have those things in place, however, we assume we will not need remote resources, futures, or streams. However, is there any way in which we could implement the rest of exorun that would make it easy to extend with these additions in the future, without overcomplicating the implementation? (five-paragraph answer)

Here is my take:

- I like the context change
- I am not sure about Transport yet. Let's keep it u8 to u8. if anything, I would likely update neorpc. That reminds me though, I would like for neorpc, in EncodedCall, to instead of taking a list of vals, to take the args as an encoded byte slice. And we can make encode return a Vec I suppose.

Can you include a section on the changes to make to neorpc? I would like to untangle the rpc mechanism from the specifics of wasmtime. neorpc should still hold the code for encoding vals, but it should be isolated in one file. 

- For ledger, I would like to keep this simple for the time being. I don't anticipate having many wire policies; but I do intend to enforce fine-grained capabilities through auth in userspace.

- My original design for transport was not request-response, hence the sequence numbers. The idea was that there may be many types of messages, and sequence numbers set up a convenient way to to refer to the same topic. Please add some instructions to verify that in call the returned reply must have matching sequence number. Maybe we make transport have (send_message) and (try_read_message) and (messages_pending -> usize) functions, which just take encoded slabs of bytes (or whatever is most reasonable), and then client manages the request-response cycle by pairing incoming responses with requests and keeping track of timeouts if a message doesn't arrive in time. Then client can be extended to support streams etc. 

- For error fidelity, that sounds important, but think of a better enum variant name than custom.
