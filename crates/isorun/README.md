# isorun

Wasmtime-based runtime for Wasm components. With this crate you can:

- Register WIT interfaces.
- Declare apps that use WIT interfaces and provide WIT interfaces.
- Provide system implementations of WIT interfaces.
- Initialize runtime pools of apps.
- Wire up apps inside a runtime pool by filling in all the interfaces.
- Run apps with budgets.
- Run apps distributed across clusters, client/server boundaries and so on.

## design considerations

I want system interfaces to be defined at compile time. So if I have an FS WIT, I can declare that at compile time, and provide a Rust implementation that uses the actual file system. However, wasm components can also provide and implement interfaces. So if I have for example a system filesystem trait and a runtime KV store trait, I could provide a wasm component that uses the FS trait and provides a KV trait, and I could use that KV trait implementation as input to another application that requires it. So it's late binding with system implementations for some fixed set of traits

I want the runtime pool to be fairly dynamic. I envision:

- Adding WIT interfaces to the pool, and getting back interface handles

- Adding wasm components (apps) to the pool if all the interfaces they depend on have been declared. This is like a prototypical copy, not a running instance!

- Being able to instantiate wasm components by providing concrete implementations for all interfaces they depend on. This returns an instance handle but does not start running things. This instance handle can be used to instantiate downstream wasm components. Additionally, if an interface is a system interface, the system handle can be used, in which case the runtime handles things. 

- Being able to run wasm components, either by calling their main function, or calling particular methods on WIT interfaces they provide. When wasm components are run, you should be able to specify a time/space budget that causes the app to exit if it's reached. I imagine we will have to think more about whether this poisons the instance, pauses execution to be resumed later, etc. 

- Being able to tear down and free wasm components once they are no longer needed, but making it an error to do that if they are being used by other components. I'm not yet sure if we one one component to run two different apps on top, that's something to think about.

I want all wasm components to only interact with host or "system" state through system interfaces defined in WIT. I'm not using WASI, just pure wasm components.

With respect to the threading model, I'd like for it to be async and non-blocking. you run an app or perform a WIT call with a fixed budget, it runs this somewhere else, you await on it and get back a final result or learn that the call timed out / ran out of space. I want the overhead of performing such calls to be pretty small.

I envision being able to do something like erlang's runtime, where components can run and communicate across multiple machines.

The basic idea is to have a webserver that has a lot of storage and so on. We have a number of system interfaces for storage, sync, public-key cryptography etc. Arbitrary users can program apps and upload them to the server. They can then implement apps that call out to the server, and the server routes their requests to their backends, which may depend on system interfaces for storage, etc. The webserver can also serve wasm components to the frontend, and we can have JS system implementations that implement e.g. storage using localstorage in the browser, and sync using websockets etc. So it's simultaneously this registry of apps, and this copyable runtime that can be deployed to the browser, of course, the browser side of things is out of score here.

But in any case, the idea is that the webserver would probably spin up a thread or task per incoming request, execute any wasm code that it needed to, and send the result back over the wire.

Eventually I want to have sync interfaces that can use e.g. webtransport so the raw wasm ABI values can be sent between client and server. Then I can implement CRDTs etc and sync them between server and client. But this webserver stuff is out of scope for now.

I think for now, with respect to resources, we ignore the idea of handles BUT we design the runtime in a way such that it is possible to implement later. It would be good to think about how we would want resources to flow between client and server or multiple servers in a cluster.

For App identifiers, I think that when we register an app, we provide the set of instance tokens that it needs to implement each interface. The runtime checks that all these instance tokens implement the correct interfaces and spawns the new instance, returning the new instance token. this instance token can be used to create other apps, or can be used to call WIT interfaces that the app implements.

We'll optimize so that we have standard system-defined interfaces like "handle request", and if an app implements such interface we can call it in a very clean way. This must be true even if the handle request interface is defined outside of this crate! For example, we define a downstream crate named isoserver that defines a static WIT interface "server", we use bindgen to generate a nice interface, and if we register an app that implements this interface the isoserver crate can use nice Rust idioms to call it.

I think for distributed resource ownership, the goal is for each app to operate mainly locally and sync that way. For example, we have a document on the server, we sync a part of that to the browser, we make changes in the browser, we sync the changes back, and we merge them in. But I don't know that's how we want to design things in the CRDT case, or whether the more general idea or having an RPC is a bad thing.

More broadly, I think that the boundary should be WIT files. So, for example, we could have a FS WIT interface, and a disk instance running on the server with a big disk. Let's call this server A. We could have another server, server B, that has a GPU. We instantiate an app on server B that needs a FS interface, and we pass the server A token for that interface. Then, when the app on server B tries to read/write data, that data is sent/requested from the FS app provider running on server A. I don't know if we want to be able to move apps, like send a running app from one computer to another, but imagine we have an app A.Q running on server A with resource handles A.1, A.2, A.3. We could transfer this app to server B, now at B.Q, by stopping A.Q, instantiating the same app with handles A.1, A.2, A.3.

For entry point, my current reasoning is that Apps require interfaces, and they may provide interfaces. We, in isorun, don't care about the particulars of this. A downstream crate could require that apps provide a main interface or a handle interface, and they can call that. But they could also provide interfaces with multiple methods, and those could be called. And again, all calls we want to run with some budget. There are a couple ways this could look like. We could have all calls take a resource parameter with time/space constraints for the individual call, or we could provision instances with storage/fuel limits, and add set storage and set fuel methods to the instances to allow the runtime to limit this.

Do we ever want to share system instances between running apps? on one hand we could have some sort of diamond dependency, where an app uses two different services that read/write to the same filesystem, and we need writes from one to be observed in reads to the other. On the other hand, we wouldn't want some app made by one user to be able to read or infer information or cause bugs in another app by exploiting some issue in a common shared system component. What gives?

Say App A and App B both use System::KVStore, and we want them to share this. Say App A is on Machine 1 and App B is on Machine 2, and we have a KV store system implementation running on Machine 1. Then we need to instantiate both App A with the local KV store implementation on Machine 1, and App B with the same cross-network Machine 1 KV store.

So we need some way to refer to machines in a cluster I suppose. I don't know if this needs to be a part of isorun; is there a way we could extend isorun to register remote services? Maybe system is sufficient in this respect. I don't want to implement the complexity of a whole RPC system. I'd like for this to be as seamless as possible.

I want to be able to run two instances concurrently. Returning to the distributed system case, we can have runtimes on different machines, but we can also have runtimes in different threads or in different processes on the same machine.

I think I want some general way to do cross-runtime requests, be in cross-network or cross-thread or cross-process. I don't want to have 1 million different RemoteX system implementations. Just one general way to take an X, make a Remote(X), and then treat that as if it were an X. If you make any calls to a remote api, we serialize according to the wasm component ABI, and emit bytes. Then a runtime can accept byte messages decode them, interpret them, and return the response. We don't include any networking code.
