// SPDX-FileCopyrightText: 2025 Robin Vobruba <hoijui.quaero@gmail.com>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use async_stream::stream;
use futures::stream::{select_all, BoxStream, Stream, StreamExt};
use tokio::time::{self, Duration};

// Define a trait with a function that returns a Stream + Unpin
trait StreamProvider {
    fn get_stream(&self) -> BoxStream<'static, u32>;
}

// First implementation of the trait
struct IncrementingStreamProvider {
    start: u32,
    step: u32,
}

impl StreamProvider for IncrementingStreamProvider {
    fn get_stream(&self) -> BoxStream<'static, u32> {
        let mut current = self.start;
        let step = self.step;

        let stream = stream! {
            loop {
                yield current;
                current += step;
                tokio::time::sleep(Duration::from_millis(800)).await;
            }
        };

        // Box the stream to make it Unpin
        stream.boxed()
    }
}

// Second implementation of the trait
struct FibonacciStreamProvider;

impl StreamProvider for FibonacciStreamProvider {
    fn get_stream(&self) -> BoxStream<'static, u32> {
        let mut a = 0;
        let mut b = 1;

        let stream = stream! {
            loop {
                yield a;
                let next = a + b;
                a = b;
                b = next;
                tokio::time::sleep(Duration::from_millis(700)).await;
            }
        };

        // Box the stream to make it Unpin
        stream.boxed()
    }
}

pub async fn test() {
    let incrementing_provider = IncrementingStreamProvider { start: 5, step: 3 };
    let fibonacci_provider = FibonacciStreamProvider;

    let incrementing_stream = incrementing_provider.get_stream();
    let fibonacci_stream = fibonacci_provider.get_stream();

    let num_streams = vec![incrementing_stream, fibonacci_stream];
    let mut combined_num_stream = select_all(num_streams);
    while let Some(value) = combined_num_stream.next().await {
        println!("Incrementing Stream: {value}");
    }
}
