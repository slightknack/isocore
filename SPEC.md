# Isocore

Isocore is the server behind `home.isaac.sh`. It is a single statically-linked binary, written in Rust, that implements all necessary functionality.

Isocore, by analogy, is like the BEAM runtime, only it uses WebAssembly (Wasm) Components and Wasm Interface Types (WIT) instead of Erlang modules.

In this document, I will outline the terminology and architecture needed to understand Isocore, along with standard interface and use cases. Isocore is a simple system, and it should be possible to implement a compatible version of Isocore from scratch by reading this document.

# 1. What is Isocore?

Isocore is a distributed runtime. It has a few unique properties:

1. All code run in Isocore is sandboxed. Programs running on Isocore can only access resources they have been granted permission to use.

2. Access to resources are determined by capabilities, or unique unforgeable references to resources. This access can be further restricted, but never broadened.

3. Programs themselves are resources, and resources can be referenced across network boundaries. This makes it possible for a program running on one machine to directly call a program running on another machine, without having to worry about serialization, authentication, etc.

Isocore is built on top of Wasmtime, an industry-standard actively-developed WebAssembly runtime, with support for both Components and WIT.

# 2. Cryptographic Background

Isocore relies on some cryptographic primitives. In this section, we explain the default cryptosystem for Isocore. The cryptosystem and interface described in this document is called:

```
iso-crypto-2026-02
```

which we will refer to as `ic2602` for short.

## 2.1 Public-Key Cryptography

`ic2602` uses `ed25519-dalek` for digital signatures and `x25519-dalek` for key exchange. Both use Curve25519; Ed25519 keys can be converted to X25519 keys. Here are the default types and interfaces, in WIT:

```wit
record key-public {
  bytes: list<u8>,  // 32 bytes
}

record key-secret {
  bytes: list<u8>,  // 32 bytes
}

// prefer key-pair over key-secret in interfaces
record key-pair {
  public: key-public,
  secret: key-secret,
}

type signature = list<u8>;  // 64 bytes (Ed25519)

generate: func() -> key-pair;
sign: func(key: key-pair, data: list<u8>) -> signature;

verify: func(key: key-public, data: list<u8>, sig: signature) -> bool;
```

## 2.2 AEAD

`ic2602` uses `XChaCha20-Poly1305` for Authenticated Encryption with Associated Data (AEAD). 

```wit
record shared-secret {
  bytes: list<u8>,  // 32 bytes
}

type text-plain = list<u8>;
type text-cipher = list<u8>;  // includes 16-byte Poly1305 tag
type random-nonce = list<u8>;  // 24 bytes (XChaCha)
// TODO: interface for secure rng, e.g. for nonce generation

// X25519 Elliptic-Curve Diffie-Hellman key exchange
conspire: func(self: key-secret, other: key-public) -> shared-secret;

encrypt: func(
  key: shared-secret, 
  nonce: random-nonce, 
  plain: text-plain, 
  metadata: text-plain
) -> text-cipher;

decrypt: func(
  key: shared-secret, 
  nonce: random-nonce, 
  cipher: text-cipher, 
  metadata: text-plain
) -> result<text-plain>;
```

## 2.3 Hashing

`ic2602` uses two rounds of `sha256` (i.e. `sha256(sha256(data))`) for secure cryptographic hashing of data, and `blake3` for fast hashing (e.g. deduplication of trusted non-adversarial data).

```wit
type hash-secure-digest = list<u8>;  // 32 bytes (sha256)
type hash-fast-digest = list<u8>;    // 32 bytes (blake3)

hash-secure: func(data: list<u8>) -> hash-secure-digest;
hash-fast: func(data: list<u8>) -> hash-fast-digest;
```

# 3. The structure of an Isocore Node

An Isocore Node runs on a single machine. The protocol and interface described in this document is called:

```
iso-node-2026-02
- iso-crypto-2026-02
```

Which we will refer to `in2602` for short. Note that `in2602` depends on `ic2602`. In the future, Nodes may support multiple cryptosystems and negotiate which one to use. For the time being, we will keep things simple.

## 3.1 Public reachability

`in2602` defines a `node` as a reachable address, along with a public key:

```wit
record node {
  node-id: key-public,
  addresses: list<node-address>,
  epoch: u64,
  signature: signature,
}

variant node-address {
  https(string),
  wss(string),
  quic(string),
}

node-publish: func(self: key-pair, addresses: list<node-address>, epoch: u64) -> node

node-update-merge: func(current: node, new: node) -> node

type connection-id = u64;

node-connect: func(
  self: key-pair,
  peer-public: key-public,
  address: node-address,
  initial-difficulty: u8,
) -> result<connection-id>;
```

A `node` may be reached through multiple addresses, but is uniquely identified by public key. It is completely acceptable for one `node` to keep multiple connections open with another `node`; connections are stateless and used purely as channels to forward encrypted messages.

`in2602` supports three types of addresses:

- `https`, for batch sending of messages through request and response, suitable for `nodes` running on the old-web.
- `wss`, for head-of-line-blocking streaming of messages and responses, suitable for `nodes` running on the old-web.
- `quic`, a modern protocol for `nodes` running on servers, without head-of-line blocking. In the future, this might open up the possibility for a `web-transport` address variant.

`nodes` may set limits on the maximum number of addresses and the maximum size of each address. Minimum acceptable limits are max 16 addresses each of max-length 256 bytes.

`node-publish` is used to update reachability. The set of valid node addresses may change over time. for this reason, the `epoch` and `signature` field exist. `epoch` is a monotonically-increasing counter. `signature` is the signature of the encoded `key-public`, `addresses` and `epoch` fields. (The exact encoding is discussed later, in X.X neopack).

`node-update-merge` is used to update a peer node when a `node` receives an announcement of a new epoch. To do so, the function validates the signature, and whether the updated `epoch` is greater than the current epoch. If so, the function replaces the previous `node` value with the new one. If validation fails, the current `node` is returned unchanged. Note that old connections do not need to be dropped on reachability update. However, if the old connections fail or time out, new connections will be established using the updated node addresses.

## 3.2 Exchanging messages

`in2602`'s approach to messaging is inspired by the noise protocol. Messages may not be any longer than 65535 bytes. Multiple messages may be packed onto the wire in sequence, though.

`in2602` is message-passing based. Here are some allowed messages:

```wit
record difficulty-params {
  difficulty: u8,
  beginning-message-epoch: u64,
}

variant node-message {
  set-difficulty(difficulty-params),
  heartbeat,
  ping,
  pong(u64),  // ping-message-epoch
  node-update(node),  // new epoch for the sending node
  // ... 
}

send-message: func(
  connection: connection-id,
  message: node-message,
) -> result<()>;
```

A message is serialized to the wire like so:

```
[ message-epoch: u64 ]
[ work-proof: u64 ]
[ length: u16 ]
[ payload: list<u8> ]
```

The `difficulty` of a message is the number of leading zeros in the hash of the whole message (all four fields above) when hashed with two rounds of `sha256` according to `ic2602`. `work-proof` is a field that can be set to any value to adjust the difficulty of a message.

The `send-message` function will serialize the message, try to find a valid value for `work-proof` to meet the connection's difficulty level. The serialized message is a `text-plain` payload. This payload is then compressed with `zstd`, encrypted using `ic2602`, and sent to the peer over whatever channel the connection provides. An error is returned if the connection was closed or a work-proof could not be generated in a reasonable amount of time.

`set-difficulty` tells another node that it will only accept messages of a given difficulty after that node's message with the given `message-epoch`. After issuing a set-difficulty message, a node may choose to drop any incoming messages with insufficient difficulty. A node should proactively issue `set-difficulty` messages to manage periods of high and low load; by default, a low difficulty, like `0` or `1` is recommended; though this should be adjusted depending on the node's base load. `set-difficulty` is the only message that must always be handled by receiving nodes regardless of its difficulty. 

`ping` and `pong` are both short messages. If a `node` receives a message `ping` with `message-epoch` N it must respond `pong(N)` as quickly as possible. `heartbeat` is like `ping` but it does not expect a reply. As these messages are meant to measure realistic round-trip times, so it is recommended that the difficulty rules are followed.

It is wise for a node to decide the max difficulty of other nodes it is willing to interact with; for example, a node running on a phone might decide not to communicate with a server node whose difficulty is above e.g. `10`, because each message would be too hard to send. A node should always pick `set-difficulty` equivalent to the maximum number of messages it is able to process to keep this value as low as possible. If a `node` expects abuse or spam, from another node, it may raise the difficulty level for that specific node. This difficulty may be asymmetrical; a client device is likely to set difficulty to zero. (Initial difficulty is communicated upon connection.) Difficulty may also be set per-peer; for example, you might choose that if your server node connects to your local node, that the bidirectional difficulty is always zero. Friends welcome, strangers knock.

`node-update` is used to inform a peer node that this node may now have new reachability requirements. When a node receives this message, it may handle it with `node-update-merge`. One important thing to note is that messages are encrypted for a specific node: so while one node may in theory proxy for another, it can only do so if the originating node announces, in an epoch, that that proxy is a valid address; the proxy may then observe the size, volume, and metadata of messages, but may not inspect their contents. 

## 3.3 Storing and publishing data

Isocore Nodes can store and publish data. The protocol is similar to DAT or hypercore for those familiar.

Data is published to Channels. A channel is a signed append-only log of events. Each event is a maximum of 65000 bytes long.

```wit
record event {
  event-id: hash-secure-digest,
  channel-id: key-public,
  event-epoch: u64,
  contents: list<u8>,  // max len 65000
}

record channel-branch {
  branch-id: hash-secure-digest,
  length: u64,
  children: list<hash-secure-digest>,  // max length 16
}

record channel-version {
  channel-id: key-public,
  event-epoch: u64,
  root-hash: hash-secure-digest,
  signature: signature,  // signature of root-hash
}

type leaf-events = list<hash-secure-digest>;

channel-append: func(channel: key-pair, event: list<u8>) -> channel-version;

// read up to `count` events starting from `epoch`
channel-get-range: func(channel: key-public, epoch: u64, count: u16) -> list<result<list<u8>>>;
```

The root hash is calculated through the formation of an implicit 16-tree. The sequence of `leaf-events` is chunked into groups of 16; the last group may have between 1..=16 hashes. All hashes in each group are concatenated in order, then hashed. This produces a sequence of `channel-branch` about 16x shorter. This process is repeated, by chunking the hashes of each `channel-branch`, until a single root `channel-branch` is formed. The `branch-id` hash of this root branch becomes the channel's root hash. This `root-hash` is then signed by the key-holder, and a new `channel-version` may be announced. Implementations should do this in an incremental manner; only recomputing the spine when a new batch of events is added.

Note, The `event-id` is computed by hashing the `contents` of the event, not including `channel-id` or `event-epoch`. The `leaf-events` are just hashes, because `channel-id` and `event-epoch` are implicit in `channel-version` based on the position in the 16-tree. This content-addressed approach leads to natural deduplication.

The `contents` of an event are application-specific. Events should be serialized using neopack (see X.X).

Channels may be updated and synchronized with these messages:

```wit
variant node-message {
  // ...
  channel-update(channel-version),
  channel-data(channel-data-payload),
  channel-subscribe(key-public),
  channel-unsubscribe(key-public),
  channel-have(event-set),
  channel-need(event-set),
  // ...
}

record channel-data-payload {
  id: hash-secure-digest,
  data: list<u8>,  // if hash-secure(data) != id, discard
}
```

An important note about node messages: Node messages are ordered, but may be delivered out of order. Messages like `subscribe` and `unsubscribe`, `have` and `need`, etc. should only be considered if they are the most recent seen message of that kind.

### 3.3.1 Event sets

An `in2602` event set is a compact way to communicate that you have or need some set of events. It has a single field, `packed`, designed to communicate sparse bitsets with choppy runs of events. Event sets are complete, meaning the packed representation contains all events up to a given version:

```wit
record event-set {
  channel: channel-version,
  packed: list<u64>,
}
```

In an event set, `1` always means "I have this event on disk" and `0` always means "I do not have this event on disk". Event sets are communicated in packed blocks 64 bits at a time. Here is the layout of a packed block:

```
DDDDDDDD DDDDDDDD DDDDDDDD DDDDDDDD 
DDDDDDDD DDDDDDDD DDDDDDDD DDDDDDTT
```

Here, `D` means data bit and `T` means tag bit. You can get all data by `>> 2` bitshifting the tag bits into the bitbucket off the end of the packed block.

- `TT == 00` means a run of `0` zero bits. The length of the run is contained in the data bits, up to `2^62-1`.
- `TT == 10` means a run of `1` one bits. The length of the run is contained in the data bits, up to `2^62-1`.
- `TT == D1` means a literal block of data bits follows. In this case, each bit should be treated as a literal `1` or `0`. `D` is treated as the 63rd data bit. This communicates the presence/absence of `63` events.

The length of the event bitset must be equal to the value of `event-epoch` in `channel-version`.

If the ending packed block is `00` or `10`, it must specify the exact remaining length. If the ending packed block is `D1`, all literal bits after the last event, based on the event bitset length, must be `0`.

### 3.3.2 Sync flow

This section describes, to a first order, how synchronization may be implemented. In future revisions of Isocore, this flow may be updated. Implementations are also free to optimize the exact ordering and priority of sent events, etc. and choosing which events to prioritize if the needed event set is very big, as long as they interoperate consistently with nodes that follow this exact default sync flow.

When a node becomes aware of a `channel-version`, it may request events from a peer node using the `channel-need` message. If the node would like to receive future events as they become available, it should send a `channel-subscribe` message before it sends `channel-need`. If a node believes that a more recent `channel-version` may be ready, it can send a `channel-need` message with zero events needed. If a node would no longer like to automatically be sent future events, it should send a `channel-unsubscribe` in addition to an empty `channel-need`.

When a `node` receives a `channel-need` message, it should always respond with a `channel-have` message, for whatever the most recent `channel-version` available is. It should prioritize specifying that it has the events the peer is requesting, or whatever the shortest true response is. (For example, if a node has all events in a channel, it can simply reply `channel-have(111...11)`.) 

`channel-need` means these events should be sent immediately if available, or as soon as available if not. Each node should maintain the most recent event set for all other nodes that have sent a channel-need, subscribed or not, and compare incoming events against that set to forward anything necessary.

### 3.3.3 Content-addressed storage

For completeness, here is the interface for a content-addressed store.

```wit
content-add: func(data: list<u8>) -> hash-secure-digest;
content-get: func(address: hash-secure-digest) -> result<list<u8>>;
```

The maximum length for an item in the store is 65000 bytes. To communicate a larger item in an event stream, a new channel should be created with each chunk of 65000 bytes added in serial order. The channel-id of this file channel should be embedded in the event.

For good compression, there is a batch variant that hashes each item individually but compresses everything together on disk. The exact compression may be implementation dependent, a good choice is e.g. zstd.

```wit
content-add-batch: func(items: list<list<u8>>) -> list<hash-secure-digest>;
```

Note that `items` is at most a batch of 16 items. Each item gets its own `hash-secure-digest` (over the uncompressed data). On disk, the batch is concatenated and compressed as a single block. Reading a single item requires decompressing the batch, but since each item is at most 65000 bytes, this is trivial.

Implementations may choose to group multiple calls to `content-add` into batches or reorganize batches on disk to improve the efficiency of the store.

## 3.4 Running applications

So far, we've established that nodes can find one another on a network, and can exchange messages in a secure manner that dynamically adjusts to load. In this section, we will talk about how Isocore runs untrusted code. We will first talk about Wasm Interface Types (WIT) and Wasm Components.

### 3.4.1 Interfaces

> Philosophical note: Isocore is a protocol-protocol. It provides a common way to specify interfaces, and how to run implementations for them. Your browser today doesn't support JPEG-XL for many reasons. Under Isocore, however, if someone writes a Component that can handle JPEG-XL images, and plugs into the existing rendering interface defined for e.g. JPEG images, individuals and existing components can opt into using this new protocol on a case-by-case basis, in a secure and sandboxed way. Isocore specifies strict limits on the size and complexity of messages and components for this reason, but as usage patterns change, I hope we can relax these limits, address any issues, and improve the runtime. As new system interfaces become available, e.g. spatial UI interfaces for Isocore on a VR device, etc., existing applications will be able to provide new presentation layers for systems that do exist at the time Isocore itself was devised.

Isocore depends heavily on interfaces, through Wasm Interface Types (WIT). An *Interface* is a collection of types and functions that describes behavior. This definition is similar to that of traits in Rust, interfaces in Go, or ABCs in Python. If you'd like a concrete example, here is a simple WIT interface we might use to describe logging.

```wit
interface logging {
  enum log-level {
    trace,
    debug,
    info,
    warn,
    error,
  }

  log: func(level: log-level, message: string);
}
```

A Wasm Component, written in any language, can call functions defined using this interface. Under the hood, WIT defines an ABI that describes how to lay out types, etc. in memory so values can cross the interface boundary.

Interfaces in `in2602` can be implemented in three ways:

- **Host** interfaces are provided by the Isocore Node itself. These are interfaces that define system capabilities, such as storing data, communicating over the network, or registering and instantiating new components.

- **Local** interfaces are provided by other Components running on the same node. For example, a Component may implement an in-memory key-value store that syncs files to disk. Other Components could use this key-value store.

- **Peer** interfaces are provided by other Isocore Nodes. These interfaces may be host interfaces or local interfaces. Peer interfaces must be published to be accessible to other nodes. Isocore generates an ephemeral public key for each published peer interface, and knowing this public key gives any node the capability to send requests to the interface.

An interface is specified using a channel-version:

```wit
record inter-version {
    suggested-name: option<human-id>,
    contents: channel-version,
}
```

Where `contents` is the textual contents of the interface. The interface is the most recent event in the channel at the specified version. Note that the id of an interface is the same as the id of the channel that defines it. 

#### 3.4.1.1 Human Ids

`human-ids` are unambiguous and easy-to-type identifiers. They are not universally unique in any capacity. 

`human-ids` must be globally-unique for any given Node, validated at creation time. Human ids map from names to hashes or public keys of different system components, and are displayed to the user when searching for or entering these otherwise nameless objects. If incoming data from a peer node includes a suggested `human-id`, which conflicts with any human ids on the current node, it is suggested that the user is presented with a box to edit the id until it is unique.

Human ids must never be stored or used as keys. They are purely a UX affordance. When a human id is entered, the corresponding key or hash must be found and used; likewise, when a hash or key is displayed to a user, the corresponding human id must be displayed.

```
// alphanumeric, no leading or trailing hyphens
// [a-z]([a-z0-9\-]*[a-z0-9])?
type human-id = string;

record pet-name {
  provenance: human-id,  // human-id of a peer node
  suggested: human-id,  // human-id used by that node
}
```

Additionally, a pet-name-like system may be enforced, where `/` is used to separate `provenance` from `suggested`. If a `node` receives a `human-id` from another node, it should look up the human id of that node, and display the combined `node/human-id` pair as a "petname" to the user. Users can either save the suggested or modified name to their own node, or choose not to. If they do not, at the UI layer, if a hash or public key can not be found locally, the best known petname may be displayed instead, as a fallback.

It is expected that this portion of `in2602` will be updated in future versions of Isocore as usage determines needs.

### 3.4.2 Components

> TODO: stylization of Component, Instance, etc.

A *Component* is a program that requires a set of Interfaces and provides implementations for a set of Interfaces. A Component is like a blueprint; to run it, you must allocate resources and wire up actual dependencies for each interface it requires. All interfaces a component requires must be satisfied before it can be run. A running Component is called an Instance. A Component Instance may depend on Host, Local, or Peer interfaces. A component may additionally provide a set of Local and/or Peer interfaces.

```wit
record component-version {
  suggested-name: option<human-id>,
  contents: channel-version,
  inter-required: list<inter-version>,
  inter-provided: list<inter-version>,
  config-schema: option<inter-version>,
}
```



TODO

In this section, we will discuss:

- what a Component is
- how components provide interfaces
- how components specify provided interfaces
- how components may start other components
- how compute and storage is managed
  - a node has strict compute and storage limits
  - these limits are allocated among applications

# Node configuration

On the spectrum from "dynamic smalltalk jungle" to "nix insect pinboard", Isocore leans much further in the nix direction. For this reason, the entire node configuration is defined in a single file, according to a WIT schema.

```
record config-node {
  node-key: key-pair,
  addresses: list<node-address>,
  instances: list<config-instance>,  
}

record config-instance {

}

record config-host {
  
}
```

TODO

- section on neopack
  - wherever we define a binary encoding, we should replace with either a wit definition or neopack, and either provide the neopack layout, or say "this type as serialized according to neopack"

TODO: future plan
- Isocore for servers
- Isocore for the old-web
- Building out home.isaac.sh with common applications
- The Internet, 2
  - Isocore interfaces for WebGPU
  - Isocore interfaces for hardware input and accessibility
  - Isocore interfaces for machine control (think MCP)
  - New browser for these universal applications.

TODO, for HN clone:

- events are application-specific messages ideally serialized with neopack
- crypto, content-addressed store, channel read/write/subscribe, network with at least one protocol supported, component lifecycle
- HN core interface; `submit-link`, `page-new`, `page-home`, `upvote`, `get-comments`, etc.
- old web gateway; http server component and interface that import the core HN interface, renders html, handles form submissions etc
  - In theory if there were a Wasm web-compatible version of isocore, the website could make these calls to the HN component interfaces directly.
- user identity, to manage keys and session tokens
- federation, subscribing to other node's HN channels, and replicating/merging links and votes across nodes
- Channel query/index interface.
- 

# Human users, machine users, and data sovereignty

- a human user is identified by a key-pair
  - issues with key management?
- we really have a few things:
  - A real human in the real world
  - An identity that they control
  - Hardware they may control and run
  - An identity that stores information on specific hardware.
- Use cases:
  - I get a new computer, and I want to use it with new or existing identities
  - I want to move my data from one computer to another
  - I lose my keys and I want to recover my account
- Some additional things:
  - I have an AI agent I want to interact with components I have running, how do I give them identity and access?
- 

# Connecting to the Old Web

---

One question I'm specifically curious about:

How should we layer capabilities and resources on top of this system?

Like:

- how does a user register a component?
- how does a user instantiate a component?
- can we change the interfaces a component accesses while it is running? is that a good idea? what if an interface crashes and we want to restart it?
- what should we do if the network fails during an RPC call? what does gRPC do, for instance?
- can we restart a component without restarting what's above it and what's below it?
- erlang has several general behaviours. for example, gen_server. I think there are 6? Can you list them all, and describe what they would look like in isocore?
- can components spawn other components? 
- in that sense, is a admin user just a special component that is authorized to perform any action in some sense?
- how do we reference a peer component? If we pass a peer component handle to someone, can they send api calls?
- let's say we have an interface with read and write. Say we want only the write method to be called. How do we restrict that? do we make a new read-only interface with a component that intercepts calls? or do we make some way to sign a subset of interface functions

- in researching these questions, which further questions do you have?
