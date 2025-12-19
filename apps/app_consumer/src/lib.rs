use wit_bindgen::generate;

generate!({
    world: "math-consumer",
    path: "../../wit",
});

struct Component;

impl exports::exorun::test::runnable::Guest for Component {
    fn run() -> String {
        // Call the OTHER app
        let result = exorun::test::math::add(10, 5);
        format!("10 + 5 = {}", result)
    }
}

export!(Component);
