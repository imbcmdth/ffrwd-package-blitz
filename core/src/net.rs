//! A `NetProvider` that serves `data:` URIs and nothing else, synchronously.
//!
//! Blitz's paint returns early while a render-blocking resource is pending,
//! and a resolve only takes in loaded resources at its start, so the bytes
//! are handed to the handler inside `fetch`: the handler decodes the image
//! (or parses the stylesheet, or registers the font) there and queues the
//! result on the document's channel, and the next `resolve`, which is the
//! one of the frame that inserted the element, picks it up.
//!
//! Every other scheme gets an empty body. That is how a refusal reaches the
//! document at all: `NetHandler` has no error path, and a handler that is
//! never called would leave a `<link>` in the head pending forever, which
//! stops the document painting. `ffrwd:` URLs (the video inputs) land here
//! too: their load fails, which leaves the element alone, and the
//! compositor hands it each frame's pixels itself.

use blitz_traits::net::{Bytes, NetHandler, NetProvider, Request};

#[derive(Default)]
pub struct DataUriProvider;

impl NetProvider for DataUriProvider {
    fn fetch(&self, _doc_id: usize, request: Request, handler: Box<dyn NetHandler>) {
        let url = request.url.to_string();
        let body = if request.url.scheme() == "data" {
            data_url::DataUrl::process(&url)
                .ok()
                .and_then(|d| d.decode_to_vec().ok())
                .map(|(bytes, _fragment)| Bytes::from(bytes))
                .unwrap_or_default()
        } else {
            Bytes::new()
        };
        handler.bytes(url, body);
    }
}
