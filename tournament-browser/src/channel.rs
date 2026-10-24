//! Events of the browser as streams, and a [`Broadcast`] channel between the tabs of the page's
//! origin.

use std::{
    pin::Pin,
    task::{Context, Poll},
};

use futures_channel::mpsc::{UnboundedReceiver, unbounded};
use futures_util::{Stream, StreamExt, future::ready};
use wasm_bindgen::{JsCast, prelude::Closure};
use web_sys::{BroadcastChannel, Event, EventTarget, MessageEvent};

/// The events of a kind that a target fires, from when this is made until it is dropped.
pub struct Events {
    target: EventTarget,
    kind: &'static str,
    listener: Closure<dyn FnMut(Event)>,
    events: UnboundedReceiver<Event>,
}

impl Events {
    /// Listens to the events `kind` of `target`, e.g. `"hashchange"` of the window.
    ///
    /// # Panics
    /// Panics if `target` takes no listener, which every target does.
    pub fn new(target: &EventTarget, kind: &'static str) -> Self {
        let (sender, events) = unbounded();
        let listener = Closure::<dyn FnMut(Event)>::new(move |event| {
            // the receiver is only dropped with the listener
            _ = sender.unbounded_send(event);
        });
        target
            .add_event_listener_with_callback(kind, listener.as_ref().unchecked_ref())
            .expect("a target takes a listener");
        Self {
            target: target.clone(),
            kind,
            listener,
            events,
        }
    }
}

impl Stream for Events {
    type Item = Event;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Event>> {
        self.events.poll_next_unpin(context)
    }
}

impl Drop for Events {
    fn drop(&mut self) {
        _ = self
            .target
            .remove_event_listener_with_callback(self.kind, self.listener.as_ref().unchecked_ref());
    }
}

/// A channel through which a tab tells the other tabs of the page's origin that listen to the
/// channel of the same name something, as a text. A tab hears nothing of what it posts itself.
pub struct Broadcast(BroadcastChannel);

impl Broadcast {
    /// Joins the channel `name`.
    ///
    /// # Panics
    /// Panics if the browser has no `BroadcastChannel`, which every one supported has.
    pub fn new(name: &str) -> Self {
        Self(BroadcastChannel::new(name).expect("the browser has broadcast channels"))
    }

    /// Tells the other tabs `message`.
    pub fn post(&self, message: &str) {
        // fails only once the channel is closed, which it is not before it is dropped
        _ = self.0.post_message(&message.into());
    }

    /// What the other tabs post from now on, as long as the stream is not dropped.
    pub fn messages(&self) -> impl Stream<Item = String> + use<> {
        Events::new(&self.0, "message").filter_map(|event| {
            ready(
                event
                    .dyn_into::<MessageEvent>()
                    .ok()
                    .and_then(|event| event.data().as_string()),
            )
        })
    }
}

impl Drop for Broadcast {
    fn drop(&mut self) {
        self.0.close();
    }
}

#[cfg(all(test, target_arch = "wasm32"))]
mod tests {
    use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

    use super::*;

    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    async fn a_message_posted_is_heard_by_the_others_on_the_channel() {
        let name = format!("channel-{}", uuid::Uuid::new_v4());
        let (one, other) = (Broadcast::new(&name), Broadcast::new(&name));
        let mut heard = Box::pin(other.messages());

        one.post("saved");

        assert_eq!(heard.next().await.as_deref(), Some("saved"));
    }
}
