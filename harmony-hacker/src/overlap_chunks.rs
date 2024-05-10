/// An iterator over a slice in overlapping (by `overlap` elements) chunks
/// (`chunk_size` elements at a time).
///
/// When the slice len is not evenly divided by the chunk size, the last slice
/// of the iteration will be the remainder.
///
/// This struct is created by the [`overlap_chunks`] method on [OverlapChunksExt].
///
/// # Example
///
/// ```
/// let slice = ['l', 'o', 'r', 'e', 'm'];
/// let iter = slice.chunks(3, 1);
/// ```
/// [`overlap_chunks`]: OverlapChunksExt::overlap_chunks
#[must_use = "iterators are lazy and do nothing unless consumed"]
#[derive(Clone)]
pub(crate) struct OverlapChunks<'a, T: 'a> {
    v: &'a [T],
    chunk_size: usize,
    next_chunk_offset: usize,
}

impl<'a, T> OverlapChunks<'a, T> {
    #[inline]
    fn new(v: &'a [T], chunk_size: usize, overlap: usize) -> Self {
        OverlapChunks {
            v,
            chunk_size,
            next_chunk_offset: chunk_size - overlap,
        }
    }
}

impl<'a, T> Iterator for OverlapChunks<'a, T> {
    type Item = &'a [T];

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.v.is_empty() {
            None
        } else if self.v.len() <= self.chunk_size {
            let chunk = self.v;
            self.v = &[];
            Some(chunk)
        } else {
            let chunk = &self.v[..self.chunk_size];
            self.v = &self.v[self.next_chunk_offset..];
            Some(chunk)
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        if self.v.is_empty() {
            (0, Some(0))
        } else {
            let overlapped = self.v.len().saturating_sub(self.chunk_size);
            let full_chunks = overlapped / (self.next_chunk_offset);
            let remainder = overlapped % (self.next_chunk_offset);
            if remainder == 0 {
                (full_chunks + 1, Some(full_chunks + 1))
            } else {
                (full_chunks + 2, Some(full_chunks + 2))
            }
        }
    }
}

/// Extension trait for slices, which adds the [`overlap_chunks`] method.
pub(crate) trait OverlapChunksExt<T> {
    /// Returns an iterator over `chunk_size` elements of the slice at a time,
    /// starting at the beginning of the slice similar to [`chunks`], but each
    /// subsequent chunk overlaps with the previous chunk by `overlap` elements.
    ///
    /// If `overlap` is 0 than the behavior is the same as [`chunks`].
    /// If `overlap` is `chunk_size - 1` than the behavior is the same as [`windows`].
    ///
    /// # Panics
    ///
    /// Panics if `overlap` is greater than or equal to `chunk_size`.
    ///
    /// # Examples
    ///
    /// ```
    /// let slice = ['l', 'o', 'r', 'e', 'm'];
    /// let mut iter = slice.overlap_chunks(3, 1);
    /// assert_eq!(iter.next().unwrap(), &['l', 'o', 'r']);
    /// assert_eq!(iter.next().unwrap(), &['r', 'e', 'm']);
    /// assert!(iter.next().is_none());
    /// ```
    /// [`chunks`]: slice::chunks
    /// [`windows`]: slice::windows
    fn overlap_chunks(&self, chunk_size: usize, overlap: usize) -> OverlapChunks<'_, T>;
}

impl<T> OverlapChunksExt<T> for [T] {
    #[inline]
    fn overlap_chunks(&self, chunk_size: usize, overlap: usize) -> OverlapChunks<'_, T> {
        assert!(overlap < chunk_size, "overlap must be less than chunk size");
        OverlapChunks::new(self, chunk_size, overlap)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn overlap_chunks_test() {
        // empty
        let slice: [i32; 0] = [];
        let mut iter = slice.overlap_chunks(1, 0);
        assert_eq!(iter.size_hint(), (0, Some(0)));
        assert!(iter.next().is_none());

        // like `windows`
        let slice = ['l', 'o', 'r', 'e', 'm'];
        let mut iter = slice.overlap_chunks(3, 2);
        assert_eq!(iter.size_hint(), (3, Some(3)));
        assert_eq!(iter.next().unwrap(), &['l', 'o', 'r']);
        assert_eq!(iter.next().unwrap(), &['o', 'r', 'e']);
        assert_eq!(iter.next().unwrap(), &['r', 'e', 'm']);
        assert!(iter.next().is_none());

        // like `chunks`
        let mut iter = slice.overlap_chunks(3, 0);
        assert_eq!(iter.size_hint(), (2, Some(2)));
        assert_eq!(iter.next().unwrap(), &['l', 'o', 'r']);
        assert_eq!(iter.next().unwrap(), &['e', 'm']);
        assert!(iter.next().is_none());

        // chunk_size > len
        let mut iter = slice.overlap_chunks(7, 2);
        assert_eq!(iter.size_hint(), (1, Some(1)));
        assert_eq!(iter.next().unwrap(), &['l', 'o', 'r', 'e', 'm']);
        assert!(iter.next().is_none());

        // overlapping
        let mut iter = slice.overlap_chunks(3, 1);
        assert_eq!(iter.size_hint(), (2, Some(2)));
        assert_eq!(iter.next().unwrap(), &['l', 'o', 'r']);
        assert_eq!(iter.next().unwrap(), &['r', 'e', 'm']);
        assert!(iter.next().is_none());

        // overlapping with remainder
        let mut iter = slice.overlap_chunks(4, 2);
        assert_eq!(iter.size_hint(), (2, Some(2)));
        assert_eq!(iter.next().unwrap(), &['l', 'o', 'r', 'e']);
        assert_eq!(iter.next().unwrap(), &['r', 'e', 'm']);
        assert!(iter.next().is_none());

        let slice = ['l', 'o', 'r', 'e', 'm', 'i', 'p', 's', 'u', 'm'];
        let mut iter = slice.overlap_chunks(6, 3);
        assert_eq!(iter.size_hint(), (3, Some(3)));
        assert_eq!(iter.next().unwrap(), &['l', 'o', 'r', 'e', 'm', 'i']);
        assert_eq!(iter.next().unwrap(), &['e', 'm', 'i', 'p', 's', 'u']);
        assert_eq!(iter.next().unwrap(), &['p', 's', 'u', 'm']);
        assert!(iter.next().is_none());
    }
}
