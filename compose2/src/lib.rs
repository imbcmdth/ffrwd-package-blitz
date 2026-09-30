//! `compose2`: 2 video inputs, and change rows from the `changes`
//! parameter and from the rows arriving with its first input's frames (the
//! host hands a module reading several streams the rows of pad 0 alone).
//!
//! Like `compose`, it keeps what the rows did from one frame to the next,
//! so it declares itself impure and runs on one worker.

compose_module::compose_module!(
    Compose2,
    compose_module::Shape {
        name: "compose2",
        inputs: 2,
        reads_rows: true,
    }
);
