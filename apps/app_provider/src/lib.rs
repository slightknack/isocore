use wit_bindgen::generate;

generate!({
    world: "math-provider",
    path: "../../wit",
});

struct Component;

impl exports::exorun::test::math::Guest for Component {
    fn add(a: u32, b: u32) -> u32 {
        a + b
    }
}

export!(Component);
