use wit_bindgen::generate;

generate!({
    world: "kv-client",
    path: "../../wit/world.wit",
});

struct Component;

impl exports::test::demo::runnable::Guest for Component {
    fn run() -> String {
        test::demo::kv::set("foo", "bar");
        let val = test::demo::kv::get("foo").unwrap_or("None".to_string());
        format!("KV result: {}", val)
    }
}

export!(Component);
