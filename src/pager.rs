//! Paginated list results, mirroring Python's `pagers.py`
//! (`Pager`/`AsyncPager`).

use std::{future::Future, pin::Pin, sync::Arc};

use futures_core::Stream;
use serde_json::{Map, Value};

use crate::error::{Error, Result};

/// Which resource kind a [`Pager`] is listing. Mirrors Python's
/// `pagers.PagedItem`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PagedItem {
    /// `client.models().list(...)`.
    Models,
    /// `client.files().list(...)`.
    Files,
    /// `client.caches().list(...)`.
    CachedContents,
    /// `client.tunings().list(...)`.
    TuningJobs,
    /// `client.batches().list(...)`.
    BatchJobs,
    /// `client.file_search_stores().list(...)`.
    FileSearchStores,
    /// `client.file_search_stores().documents().list(...)`.
    Documents,
}

type FetchPage<T> = Arc<
    dyn Fn(
            Map<String, Value>,
        ) -> Pin<Box<dyn Future<Output = Result<(Vec<T>, Option<String>)>> + Send>>
        + Send
        + Sync,
>;

/// A single page of a listing endpoint, with the ability to fetch
/// subsequent pages. Mirrors Python's `Pager`/`AsyncPager`.
pub struct Pager<T> {
    name: PagedItem,
    page: Vec<T>,
    config: Map<String, Value>,
    next_page_token: Option<String>,
    fetch: FetchPage<T>,
}

impl<T: std::fmt::Debug> std::fmt::Debug for Pager<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pager")
            .field("name", &self.name)
            .field("page", &self.page)
            .field("config", &self.config)
            .field("next_page_token", &self.next_page_token)
            .finish_non_exhaustive()
    }
}

impl<T> Pager<T> {
    /// Constructs a [`Pager`] from the first page's items, the resolved
    /// `next_page_token`, and a closure that fetches subsequent pages
    /// given an updated config (with `page_token` set).
    pub(crate) fn new(
        name: PagedItem,
        page: Vec<T>,
        config: Map<String, Value>,
        next_page_token: Option<String>,
        fetch: FetchPage<T>,
    ) -> Self {
        Self {
            name,
            page,
            config,
            next_page_token,
            fetch,
        }
    }

    /// Which resource kind this pager lists.
    #[must_use]
    pub fn name(&self) -> PagedItem {
        self.name
    }

    /// The current page's items.
    #[must_use]
    pub fn page(&self) -> &[T] {
        &self.page
    }

    /// The current page's size (item count), as configured or observed.
    #[must_use]
    pub fn page_size(&self) -> usize {
        self.config
            .get("page_size")
            .and_then(Value::as_u64)
            .map_or(self.page.len(), |n| {
                usize::try_from(n).unwrap_or(usize::MAX)
            })
    }

    /// The request config used to fetch the current page (includes
    /// `page_token` once advanced).
    #[must_use]
    pub fn config(&self) -> &Map<String, Value> {
        &self.config
    }

    /// Fetches and returns the next page, replacing the current one.
    ///
    /// # Errors
    /// Returns [`Error::NoMorePages`] if there is no further page.
    pub async fn next_page(&mut self) -> Result<&[T]> {
        let Some(token) = self.next_page_token.clone() else {
            return Err(Error::NoMorePages);
        };
        let mut config = self.config.clone();
        config.insert("page_token".to_owned(), Value::String(token));
        let (page, next_token) = (self.fetch)(config.clone()).await?;
        self.page = page;
        self.config = config;
        self.next_page_token = next_token;
        Ok(&self.page)
    }

    /// Consumes this pager, returning a stream of every item across every
    /// page (including the current one).
    #[must_use = "returns a lazy Stream; nothing is fetched until it is polled"]
    pub fn into_stream(self) -> impl Stream<Item = Result<T>>
    where
        T: Send + 'static,
    {
        async_stream::try_stream! {
            let mut pager = self;
            for item in std::mem::take(&mut pager.page) {
                yield item;
            }
            loop {
                match pager.next_page().await {
                    Ok(_) => {
                        for item in std::mem::take(&mut pager.page) {
                            yield item;
                        }
                    }
                    Err(Error::NoMorePages) => break,
                    Err(err) => Err(err)?,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use futures_util::StreamExt;
    use serde_json::{Map, Value, json};

    use super::{PagedItem, Pager};
    use crate::error::Error;

    fn two_page_pager() -> Pager<i32> {
        let calls = std::sync::Arc::new(AtomicUsize::new(0));
        Pager::new(
            PagedItem::Files,
            vec![1, 2],
            Map::new(),
            Some("tok1".to_owned()),
            std::sync::Arc::new(move |config: Map<String, Value>| {
                let calls = calls.clone();
                Box::pin(async move {
                    let call = calls.fetch_add(1, Ordering::SeqCst);
                    assert_eq!(call, 0, "only one further page should be fetched");
                    assert_eq!(config.get("page_token"), Some(&json!("tok1")));
                    Ok((vec![3, 4], None))
                })
            }),
        )
    }

    #[tokio::test]
    async fn next_page_advances_and_then_errors_when_exhausted() {
        let mut pager = two_page_pager();
        let second = pager.next_page().await.unwrap();
        assert_eq!(second, &[3, 4]);
        let err = pager.next_page().await.unwrap_err();
        assert!(matches!(err, Error::NoMorePages));
    }

    #[tokio::test]
    async fn into_stream_yields_every_item_across_pages() {
        let pager = two_page_pager();
        let items: Vec<i32> = pager.into_stream().map(|r| r.unwrap()).collect().await;
        assert_eq!(items, vec![1, 2, 3, 4]);
    }
}
