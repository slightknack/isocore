use wit_bindgen::generate;

generate!({
    world: "kv-client",
    path: "../../wit",
    generate_all,
});

struct Component;

impl exports::exorun::test::runnable::Guest for Component {
    fn run() -> String {
        exorun::host::kv::set("foo", "bar");
        let val = exorun::host::kv::get("foo").unwrap_or("None".to_string());
        format!("KV result: {}", val)
    }
}

export!(Component);
