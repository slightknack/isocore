---
origin: handwritten
date: 2025-12-10
---

# Design

home.isaac.sh is a website for discussion. It aims to facilitate the following interactions:

- Discovery of current, interesting written content.
- Conversations between smart and thoughtful people.
- Publishing and dissemination of interesting ideas.
- Collaborative development of local-first software.

What's in an app?

An app is a program compiled to WebAssembly, that interacts with a number of interfaces, which may be defined by the system or the user. To that end we use the component model. Applications are run per-user. Interfaces include:

- Identity, which allows you to read the user's public key, generate ephemeral private keys, sign & verify and encrypt & decrypt messages.
- Phonebook, which allows looking up other users and their key material.
- Core, which allows you to maintain an append-only log of messages.
- Talk, which implements sync on top of core. (Which can be used for CRDTs.)
- Storage, for file storage.
- Serve, which allows static storage volumes to be served.
- Web, which allows receive and process requests.
- Meta, which allows publishing new apps and so on.

The Rust binary is fairly simple:

- It embeds Wasmtime for running apps.
- It embeds SQLite for interacting with databases.
- It implements system interfaces and capabilities.
- It runs a webserver for processing and routing requests.

We also provide a default app which runs the default web gateway.

## First behaviours

I need to be able to:

- Maintain a list of users, their keys, and allow others to invite users and allow invited users to sign up.
  - list of users can be in a core
-
