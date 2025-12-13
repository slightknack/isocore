<!-- handwritten -->
<!-- 2025-12-12 -->

We have:

- System
- LocalInstance (note let's rename this to Local and link_local_instance to link_local)
- Remote

So. For local instances forwarding WIT calls should be trivial, we just have to cross a store boundary.

For remote instances we need to instrospect, serialize, write that to some generic transport interface, and yield. 

- When our rpc listener receives a reply:
  - If success we load the value and the await point resumes
  - If failure we indicate as such and trap at the await point to tell the app there was a failure or return an error
  - We should probably also set a timeout so apps don't hang indefinitely.

For system interfaces this is just manual and static and we can bend to wasmtime's will.
