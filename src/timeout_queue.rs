use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, Mutex};
use tokio::timer::Timeout;

#[derive(Debug, Eq, PartialEq)]
enum TimeoutQueueMsg {
    Wake,
    RePoll,
}

/// A Timeout queue
///
/// Mostly adapted from [`fibers_timeout_queue`]
///
/// [`fibers_timeout_queue`]: https://github.com/sile/fibers_timeout_queue
#[derive(Debug)]
pub struct TimeoutQueue<T> {
    queue: Mutex<BinaryHeap<Item<T>>>,
    enqueue_lock: Mutex<mpsc::Sender<TimeoutQueueMsg>>,
    dequeue_lock: Mutex<mpsc::Receiver<TimeoutQueueMsg>>,
}

#[derive(Debug)]
struct Item<T> {
    expiry_instant: Instant,
    item: T,
}

impl<T> TimeoutQueue<T> {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel(1);

        TimeoutQueue {
            queue: Mutex::new(BinaryHeap::new()),
            enqueue_lock: Mutex::new(tx),
            dequeue_lock: Mutex::new(rx),
        }
    }

    pub async fn push(&self, value: T, timeout: Duration) {
        let mut wakeup_send = self.enqueue_lock.lock().await;
        let expiry_instant = Instant::now() + timeout;

        let mut queue = self.queue.lock().await;

        // we want to trigger wakeups if the queue was empty
        let should_trigger_wakeup = queue
            .peek()
            .map_or(true, |x| expiry_instant < x.expiry_instant);

        queue.push(Item {
            expiry_instant,
            item: value,
        });

        if should_trigger_wakeup {
            wakeup_send
                .send(TimeoutQueueMsg::RePoll)
                .await
                .expect("TimeoutQueue waker dropped when it shouldn't");
        }
    }

    pub async fn pop(&self) -> T {
        let mut wakeup_recv = self.dequeue_lock.lock().await;

        loop {
            if let Some(Item {
                item: value,
                expiry_instant,
            }) = self.queue.lock().await.pop()
            {
                if let Ok(Some(_msg)) = Timeout::new_at(wakeup_recv.recv(), expiry_instant).await {
                    continue;
                } else {
                    // timeout expired, return value
                    return value;
                }
            } else {
                // no items left, go to sleep
                let res = wakeup_recv.recv().await;
                assert_eq!(res, Some(TimeoutQueueMsg::Wake));
            }
        }
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
