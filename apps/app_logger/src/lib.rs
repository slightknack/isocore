use wit_bindgen::generate;

generate!({
    world: "logger-client",
    path: "../../wit",
    generate_all,
});

struct Component;

impl exports::exorun::test::runnable::Guest for Component {
    fn run() -> String {
        // Call the host system interface
        exorun::host::logging::log("INFO", "Hello from Wasm!");
        "Done".to_string()
    }
}

export!(Component);
