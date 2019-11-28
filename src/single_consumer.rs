use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Something that can hold a value and one person can take it out
#[derive(Clone)]
pub struct SingleConsumer<T> {
    inner: Arc<SingleConsumerInner<T>>,
}

struct SingleConsumerInner<T> {
    taken: AtomicBool,
    val: UnsafeCell<Option<T>>,
}

unsafe impl<T: Send> Send for SingleConsumerInner<T> {}
unsafe impl<T: Send> Sync for SingleConsumerInner<T> {}

impl<T> SingleConsumerInner<T> {
    fn new(val: T) -> Self {
        SingleConsumerInner {
            taken: AtomicBool::new(false),
            val: UnsafeCell::new(Some(val)),
        }
    }

    fn take(&self) -> Option<T> {
        if self
            .taken
            .compare_exchange_weak(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            // this is okay, only ever entered once
            unsafe { self.val.get().replace(None) }
        } else {
            None
        }
    }
}

impl<T> SingleConsumer<T> {
    pub fn new(val: T) -> Self {
        SingleConsumer {
            inner: Arc::new(SingleConsumerInner::new(val)),
        }
    }

    pub fn take(&self) -> Option<T> {
        self.inner.take()
    }
}

impl<T> std::fmt::Debug for SingleConsumer<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "SingleConsumer<{}>()", std::any::type_name::<T>())
    }
}
