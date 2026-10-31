//! Tiny scoped-thread helpers for "go wide, come back this frame" work such as
//! find and note search. No pool, no dependency: threads are cheap compared with
//! the multi-megabyte scans they split, and small inputs stay on the caller thread.

use std::num::NonZeroUsize;

pub(crate) fn worker_count() -> usize {
    std::thread::available_parallelism()
        .map(NonZeroUsize::get)
        .unwrap_or(1)
        .clamp(1, 16)
}

/// Maps `f` over `items` on up to [`worker_count`] threads, preserving order.
pub(crate) fn par_map<T, R, F>(items: &[T], f: F) -> Vec<R>
where
    T: Sync,
    R: Send,
    F: Fn(&T) -> R + Sync,
{
    let workers = worker_count().min(items.len());
    if workers <= 1 {
        return items.iter().map(&f).collect();
    }
    let chunk = items.len().div_ceil(workers);
    std::thread::scope(|scope| {
        let handles = items
            .chunks(chunk)
            .map(|part| {
                let f = &f;
                scope.spawn(move || part.iter().map(f).collect::<Vec<_>>())
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .flat_map(|handle| handle.join().expect("parallel worker panicked"))
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::par_map;

    #[test]
    fn par_map_preserves_order() {
        let items = (0..10_000).collect::<Vec<u32>>();
        let doubled = par_map(&items, |n| n * 2);
        assert_eq!(doubled, items.iter().map(|n| n * 2).collect::<Vec<_>>());
    }

    #[test]
    fn par_map_handles_empty_and_single_inputs() {
        assert!(par_map(&[] as &[u8], |n| *n).is_empty());
        assert_eq!(par_map(&[7u8], |n| *n + 1), vec![8]);
    }
}
