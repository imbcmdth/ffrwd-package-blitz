//! `compose1`: 1 video input(s), change rows from the `changes` parameter
//! only. Nothing but the parameters drives the document, so every instance
//! renders any frame the same and the host may spread frames over workers.

compose_module::compose_module!(
    Compose1,
    compose_module::Shape {
        name: "compose1",
        inputs: 1,
        reads_rows: false,
    }
);
