mod bindings {
    wit_bindgen::generate!({
        path: "../engine-wit",
        with: { "dekopon:engine/api@0.1.0": generate },
        world: "engine-provider",
        pub_export_macro: true,
    });
}

use bindings::exports::dekopon::engine::api::{Guest, GuestCounter};
use std::sync::atomic::{AtomicU32, Ordering};

static LIVE: AtomicU32 = AtomicU32::new(0);

struct Engine;
struct Counter;

impl Guest for Engine {
    type Counter = Counter;

    fn engine_name() -> String {
        "engine-a".into()
    }

    fn live_count() -> u32 {
        LIVE.load(Ordering::SeqCst)
    }

    fn burn(iterations: u32) -> u32 {
        let mut value = 0_u32;
        for _ in 0..iterations {
            value = std::hint::black_box(value).wrapping_add(1);
        }
        value
    }

    fn allocate(bytes: u32) -> u32 {
        let mut buffer = vec![0_u8; bytes as usize];
        std::hint::black_box(buffer.as_mut_slice());
        buffer.len() as u32
    }
}

impl GuestCounter for Counter {
    fn new() -> Self {
        LIVE.fetch_add(1, Ordering::SeqCst);
        Self
    }

    fn get(&self) -> String {
        "engine-a".into()
    }
}

impl Drop for Counter {
    fn drop(&mut self) {
        LIVE.fetch_sub(1, Ordering::SeqCst);
    }
}

bindings::export!(Engine with_types_in bindings);
