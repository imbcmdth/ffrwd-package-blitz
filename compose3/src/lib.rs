//! `compose3`: 3 video input(s), change rows from the `changes` parameter
//! only. Nothing but the parameters drives the document, so every instance
//! renders any frame the same and the host may spread frames over workers.

compose_module::compose_module!(
    Compose3,
    compose_module::Shape {
        name: "compose3",
        inputs: 3,
        reads_rows: false,
    }
);
