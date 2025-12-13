---
origin: handwritten
date: 2025-12-12
---

I would like to be able to do the following:

- register a wasm component
  - wasm components communicate through WIT.
  - wasm components declare a set of WIT interfaces they need.
  - you can instantiate a wasm component by linking all its interfaces.
- I would like to link the following types of instances when instantiating a component:
  - system instances, implemented directly in Rust
  - local instances, implemented by other wasm components
  - remote instances, which may be called through neorpc
- this requires that, when we link an instance:
  - we record all interfaces that it implements and requires
  - we record all functions, input types, and return types used by these interfaces
- when we call a remote instance from a wasm component, the linked interface:
  - captures the arguments as a list of wasmtime Val
  - serializes the arguments into a message using neorpc
  - serializes the message into a frame using neorpc
  - sends this frame over some generic transport channel
  - waits for a response, be it success or failure
  - if success:
    - decodes the return frame into a message
    - decodes the message into a list of wasm vals
    - resumes the application with those vals
  - if failure:
    - decodes the return frame to figure out the failure reason
    - traps or otherwise communicates the failure to the application
- this is similar for system and remote instances, but not that no serialization over neorpc is needed

The all relevant parts of the wasmtime implementation have to do with Val, Types, Components and Instances.
