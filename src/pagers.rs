//! Paginated list results, mirroring Python's `pagers.py`
//! (`Pager`/`AsyncPager`).

use std::{future::Future, pin::Pin, sync::Arc};

use futures_core::Stream;
use serde_json::{Map, Value};

use crate::{
    errors::{Error, Result},
    types::HttpResponse,
};

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

/// One fetched page of a listing: its items, the token for the page after
/// it, and the HTTP response (headers) the page came from.
#[derive(Debug)]
pub(crate) struct Page<T> {
    /// The page's items.
    pub items: Vec<T>,
    /// Token of the following page, if any.
    pub next_page_token: Option<String>,
    /// Python's `response.sdk_http_response` of the list call.
    pub sdk_http_response: Option<HttpResponse>,
}

pub(crate) type FetchPage<T> = Arc<
    dyn Fn(Map<String, Value>) -> Pin<Box<dyn Future<Output = Result<Page<T>>> + Send>>
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
    sdk_http_response: Option<HttpResponse>,
    fetch: FetchPage<T>,
}

impl<T: std::fmt::Debug> std::fmt::Debug for Pager<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pager")
            .field("name", &self.name)
            .field("page", &self.page)
            .field("config", &self.config)
            .field("next_page_token", &self.next_page_token)
            .field("sdk_http_response", &self.sdk_http_response)
            .finish_non_exhaustive()
    }
}

impl<T> Pager<T> {
    /// Constructs a [`Pager`] from the first fetched [`Page`] and a closure
    /// that fetches subsequent pages given an updated config (with
    /// `page_token` set).
    pub(crate) fn new(
        name: PagedItem,
        first: Page<T>,
        config: Map<String, Value>,
        fetch: FetchPage<T>,
    ) -> Self {
        Self {
            name,
            page: first.items,
            config,
            next_page_token: first.next_page_token,
            sdk_http_response: first.sdk_http_response,
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

    /// The HTTP response (headers) of the API response that produced the
    /// current page; follows [`Self::next_page`]. Mirrors Python's
    /// `_BasePager.sdk_http_response`. `None` when the list call does not
    /// report one (Python leaves it unset for `file_search_stores.list` and
    /// `documents.list`).
    #[must_use]
    pub fn sdk_http_response(&self) -> Option<&HttpResponse> {
        self.sdk_http_response.as_ref()
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
        let next = (self.fetch)(config.clone()).await?;
        self.page = next.items;
        self.config = config;
        self.next_page_token = next.next_page_token;
        self.sdk_http_response = next.sdk_http_response;
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

    use super::{Page, PagedItem, Pager};
    use crate::{errors::Error, types::HttpResponse};

    fn response_with_header(value: &str) -> HttpResponse {
        HttpResponse {
            headers: Some(std::collections::HashMap::from([(
                "x-page".to_owned(),
                value.to_owned(),
            )])),
            body: None,
        }
    }

    fn two_page_pager() -> Pager<i32> {
        let calls = std::sync::Arc::new(AtomicUsize::new(0));
        Pager::new(
            PagedItem::Files,
            Page {
                items: vec![1, 2],
                next_page_token: Some("tok1".to_owned()),
                sdk_http_response: Some(response_with_header("first")),
            },
            Map::new(),
            std::sync::Arc::new(move |config: Map<String, Value>| {
                let calls = calls.clone();
                Box::pin(async move {
                    let call = calls.fetch_add(1, Ordering::SeqCst);
                    assert_eq!(call, 0, "only one further page should be fetched");
                    assert_eq!(config.get("page_token"), Some(&json!("tok1")));
                    Ok(Page {
                        items: vec![3, 4],
                        next_page_token: None,
                        sdk_http_response: Some(response_with_header("second")),
                    })
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

    #[test]
    fn sdk_http_response_exposes_the_first_pages_headers() {
        let pager = two_page_pager();
        assert_eq!(
            pager.sdk_http_response(),
            Some(&response_with_header("first"))
        );
    }

    #[tokio::test]
    async fn sdk_http_response_follows_the_current_page_after_next_page() {
        let mut pager = two_page_pager();
        pager.next_page().await.unwrap();
        assert_eq!(
            pager.sdk_http_response(),
            Some(&response_with_header("second"))
        );
    }

    #[test]
    fn sdk_http_response_is_none_when_the_list_call_reports_none() {
        let pager: Pager<i32> = Pager::new(
            PagedItem::Documents,
            Page {
                items: vec![],
                next_page_token: None,
                sdk_http_response: None,
            },
            Map::new(),
            std::sync::Arc::new(|_| Box::pin(async { Err(Error::NoMorePages) })),
        );
        assert_eq!(pager.sdk_http_response(), None);
    }
}
