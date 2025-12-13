<!-- handwritten -->
<!-- 2025-12-12 -->

Taking a step back, how do we want to use components defined on other machines?

Imagine I have an app running on machine 1 that provides implementations for two interfaces:

- kv
- log

under the hood, say, this app writes kv entries and log entries to the same file system.

Then I have another app, app B, running on machine 2, that requires 1 interface: kv.

I can register app A on machine 1 as a dependency of app B if I have machine 1 as a peer. That looks something like:

```
let app_A_addr = RemoteAddr {
  peer: machine_1,
  target_id: "app_A",
};

let app_B_inst = InstanceBuilder::new(&rt, app_B_id)
  .link_remote("my-org:kv", app_A_addr).await?
  .instantiate().await?;
```

That's pretty cool!
