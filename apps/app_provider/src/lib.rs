use wit_bindgen::generate;

generate!({
    world: "math-provider",
    path: "../../wit/world.wit",
});

struct Component;

impl exports::test::demo::math::Guest for Component {
    fn add(a: u32, b: u32) -> u32 {
        a + b
    }
}

export!(Component);
