//! `compose`: one video input, and change rows from the `changes`
//! parameter and from the rows arriving with its frames.
//!
//! It keeps what the rows did from one frame to the next, and a host
//! spreading frames over workers hands each worker only its own frames'
//! rows, so it declares itself impure and runs on one worker.
//!
//! TODO(row history): once ffrwd's sidecar appends the rows of the frames a
//! worker skipped to the next frame it hands that worker (the module opting
//! in through the export the host branch settles on), set `reads_rows`
//! aside from purity here and declare `pure: true`. `Session` already
//! applies such rows in `at` order, which is all the module side needs.

compose_module::compose_module!(
    Compose,
    compose_module::Shape {
        name: "compose",
        inputs: 1,
        reads_rows: true,
    }
);
