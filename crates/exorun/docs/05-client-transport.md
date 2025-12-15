---
origin: handwritten
date: 2025-12-15
---

The original idea of adding client and making transport send and receive was that in the future, for things like streams, we actually want a message-centric way of doing things. and then client just adds a request-response layer on top of transport for doing that.

So this is what made it natural for client to hold onto sequence numbers. But the problem is that multiple clients may share the same underlying transports.

So one option is to make client the thing we hold in a remote target, and what we return when we construct a peer. 

That way clients are shared and messages are ordered sequentially by the client.

---

Let's assume we're running a collection of apps, and we lose a connection to a peer temporarily. How could we reconnect? Would this need to be handled at the transport layer?

What if a machine went offline, and we want to connect to it at a different address, or over a different protocol?

---

one thing I want two be able to do is write a meta WIT interface that allows other apps to register new apps and spin up other instances and so on. So adding the runtime to the context makes sense.
