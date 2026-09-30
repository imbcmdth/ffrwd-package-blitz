//! `compose2`: 2 video input(s), change rows from the `changes` parameter
//! only. Nothing but the parameters drives the document, so every instance
//! renders any frame the same and the host may spread frames over workers.

compose_module::compose_module!(
    Compose2,
    compose_module::Shape {
        name: "compose2",
        inputs: 2,
        reads_rows: false,
    }
);
