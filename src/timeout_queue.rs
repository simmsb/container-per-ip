use std::cmp::Ordering;
use std::time::{Duration, Instant};
use std::collections::BinaryHeap;

use tokio::sync::{mpsc, Mutex};
use tokio::timer::Timeout;

#[derive(Debug, Eq, PartialEq)]
enum TimeoutQueueMsg {
    Wake,
    RePoll,
    Discard,
}

/// A Timeout queue with item keys that lets you cancel the timeout of an item
///
/// Mostly adapted from [`fibers_timeout_queue`]
///
/// [`fibers_timeout_queue`]: https://github.com/sile/fibers_timeout_queue
#[derive(Debug)]
pub struct CancellableTimeoutQueue<K, T> {
    queue: Mutex<BinaryHeap<Item<(K, T)>>>,
    current_waiting: Option<K>,
    wakeup_recv: mpsc::Receiver<TimeoutQueueMsg>,
    wakeup_send: mpsc::Sender<TimeoutQueueMsg>,
    dequeue_lock: Mutex<()>,
}

#[derive(Debug)]
struct Item<T> {
    expiry_instant: Instant,
    item: T,
}

impl<K: PartialEq, T> CancellableTimeoutQueue<K, T> {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel(1);

        CancellableTimeoutQueue {
            queue: Mutex::new(BinaryHeap::new()),
            current_waiting: None,
            wakeup_recv: rx,
            wakeup_send: tx,
            dequeue_lock: Mutex::new(()),
        }
    }

    pub async fn push(&mut self, key: K, value: T, timeout: Duration) {
        let expiry_instant = Instant::now() + timeout;

        let mut queue = self.queue.lock().await;

        // we want to trigger wakeups if the queue was empty
        let should_trigger_wakeup = queue
            .peek()
            .map_or(true, |x| expiry_instant < x.expiry_instant);

        queue.push(Item { expiry_instant, item: (key, value) });

        if should_trigger_wakeup {
            self.wakeup_send.send(TimeoutQueueMsg::RePoll).await
                .expect("TimeoutQueue waker dropped when it shouldn't");
        }
    }

    pub async fn pop(&mut self) -> T {
        let _lock = self.dequeue_lock.lock().await;

        loop {
            if let Some(Item { item: (key, value), expiry_instant }) = self.queue.lock().await.pop() {
                self.current_waiting = Some(key);

                if let Ok(Some(msg)) = Timeout::new_at(self.wakeup_recv.recv(), expiry_instant).await {
                    // woken up, reinsert and reloop

                    let key = self.current_waiting
                                  .take()
                                  .expect("current key removed when it should be there.");
                    if msg != TimeoutQueueMsg::Discard {
                        self.queue.lock().await.push(Item { expiry_instant, item: (key, value) });
                    }
                    continue;
                } else {
                    // timeout expired, return value
                    self.current_waiting.take();
                    return value;
                }
            } else {
                // no items left, go to sleep

                let res = self.wakeup_recv.recv().await;
                assert_eq!(res, Some(TimeoutQueueMsg::Wake));
            }
        }
    }

    pub async fn cancel(&mut self, key: K) {
        if self.current_waiting.as_ref() == Some(&key) {
            self.wakeup_send.send(TimeoutQueueMsg::Discard)
                            .await.expect("TimeoutQueue waker dropped when it shouldn't");
        }

        let mut queue = self.queue.lock().await;

        let vec = queue.drain().filter(|item| item.item.0 != key).collect::<Vec<_>>();

        queue.extend(vec);
    }
}


impl<T> PartialOrd for Item<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        other.expiry_instant.partial_cmp(&self.expiry_instant)
    }
}

impl<T> Ord for Item<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        other.expiry_instant.cmp(&self.expiry_instant)
    }
}

impl<T> PartialEq for Item<T> {
    fn eq(&self, other: &Self) -> bool {
        self.expiry_instant == other.expiry_instant
    }
}

impl<T> Eq for Item<T> {}
