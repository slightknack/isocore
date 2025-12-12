use wit_bindgen::generate;

generate!({
    world: "logger-client",
    path: "../../wit/world.wit",
});

struct Component;

impl exports::test::demo::runnable::Guest for Component {
    fn run() -> String {
        // Call the host system interface
        test::demo::logging::log("INFO", "Hello from Wasm!");
        "Done".to_string()
    }
}

export!(Component);
