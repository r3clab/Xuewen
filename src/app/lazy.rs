use std::sync::OnceLock;

type Init<T, E> = Box<dyn Fn() -> Result<T, E> + Send + Sync>;

pub(super) enum Lazy<T, E = anyhow::Error> {
    Ready(T),
    Deferred {
        value: OnceLock<T>,
        init: Init<T, E>,
    },
}

impl<T, E> Lazy<T, E> {
    pub(super) fn deferred(init: impl Fn() -> Result<T, E> + Send + Sync + 'static) -> Self {
        Lazy::Deferred {
            value: OnceLock::new(),
            init: Box::new(init),
        }
    }

    pub(super) fn get(&self) -> Result<&T, E> {
        match self {
            Lazy::Ready(value) => Ok(value),
            Lazy::Deferred { value, init } => {
                if let Some(v) = value.get() {
                    return Ok(v);
                }
                let v = init()?;
                Ok(value.get_or_init(|| v))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn deferred_retries_failures_and_caches_the_first_success() {
        let calls = Arc::new(AtomicUsize::new(0));
        let lazy: Lazy<usize, String> = Lazy::deferred({
            let calls = calls.clone();
            move || match calls.fetch_add(1, Ordering::SeqCst) {
                0 => Err("not yet".to_string()),
                n => Ok(n),
            }
        });
        assert_eq!(lazy.get(), Err("not yet".to_string()));
        assert_eq!(lazy.get(), Ok(&1));
        assert_eq!(lazy.get(), Ok(&1));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(Lazy::<_, String>::Ready(7).get(), Ok(&7));
    }
}
