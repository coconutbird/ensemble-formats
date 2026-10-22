//! Real-file glTF roundtrip tests using game data supplied through environment variables.

mod compare;
mod cross_version;
mod hogan;
mod sources;

/// Select at most `limit` items at evenly spaced indices, including the first and the last item.
///
/// If `items` has more than `limit` items and `limit` is 0 or 1, the result contains only the
/// first item.
fn evenly_spaced<T>(items: &[T], limit: usize) -> Vec<&T> {
    if items.len() <= limit {
        return items.iter().collect();
    }
    if limit <= 1 {
        return items.first().into_iter().collect();
    }
    (0..limit)
        .map(|index| &items[index * (items.len() - 1) / (limit - 1)])
        .collect()
}
