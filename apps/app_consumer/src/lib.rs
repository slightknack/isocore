use wit_bindgen::generate;

generate!({
    world: "math-consumer",
    path: "../../wit/world.wit",
});

struct Component;

impl exports::test::demo::runnable::Guest for Component {
    fn run() -> String {
        // Call the OTHER app
        let result = test::demo::math::add(10, 5);
        format!("10 + 5 = {}", result)
    }
}

export!(Component);
