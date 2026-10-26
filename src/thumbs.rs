//! Loads thumbnails over HTTPS through fast.yt's own HTTP client, which trusts the operating
//! system's certificates. egui's built-in HTTP loader only trusts a bundled list, so thumbnails
//! fail behind corporate TLS inspection.

use crate::yt;
use eframe::egui::{
    self,
    load::{Bytes, BytesLoadResult, BytesLoader, BytesPoll, LoadError},
    mutex::Mutex,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::task::Poll;

type Entry = Poll<Result<Arc<[u8]>, String>>;

#[derive(Default)]
pub struct ThumbnailLoader {
    cache: Arc<Mutex<HashMap<String, Entry>>>,
}

impl ThumbnailLoader {
    pub const ID: &'static str = egui::generate_loader_id!(ThumbnailLoader);
}

fn fetch(uri: &str) -> Result<Arc<[u8]>, String> {
    let mut response = yt::http().get(uri).call().map_err(|e| format!("failed to load {uri:?}: {e}"))?;
    let bytes = response
        .body_mut()
        .with_config()
        .limit(10 * 1024 * 1024)
        .read_to_vec()
        .map_err(|e| format!("failed to read {uri:?}: {e}"))?;
    Ok(bytes.into())
}

impl BytesLoader for ThumbnailLoader {
    fn id(&self) -> &str {
        Self::ID
    }

    fn load(&self, ctx: &egui::Context, uri: &str) -> BytesLoadResult {
        if !(uri.starts_with("https://") || uri.starts_with("http://")) {
            return Err(LoadError::NotSupported);
        }
        let mut cache = self.cache.lock();
        match cache.get(uri) {
            Some(Poll::Ready(Ok(bytes))) => {
                Ok(BytesPoll::Ready { size: None, bytes: Bytes::Shared(Arc::clone(bytes)), mime: None })
            }
            Some(Poll::Ready(Err(err))) => Err(LoadError::Loading(err.clone())),
            Some(Poll::Pending) => Ok(BytesPoll::Pending { size: None }),
            None => {
                cache.insert(uri.to_owned(), Poll::Pending);
                drop(cache);
                let (uri, cache, ctx) = (uri.to_owned(), Arc::clone(&self.cache), ctx.clone());
                std::thread::spawn(move || {
                    let result = fetch(&uri);
                    let still_wanted = match cache.lock().get_mut(&uri) {
                        Some(entry) => {
                            *entry = Poll::Ready(result);
                            true
                        }
                        None => false,
                    };
                    if still_wanted {
                        ctx.request_repaint();
                    }
                });
                Ok(BytesPoll::Pending { size: None })
            }
        }
    }

    fn forget(&self, uri: &str) {
        self.cache.lock().remove(uri);
    }

    fn forget_all(&self) {
        self.cache.lock().clear();
    }

    fn byte_size(&self) -> usize {
        self.cache
            .lock()
            .values()
            .map(|entry| match entry {
                Poll::Ready(Ok(bytes)) => bytes.len(),
                Poll::Ready(Err(err)) => err.len(),
                Poll::Pending => 0,
            })
            .sum()
    }

    fn has_pending(&self) -> bool {
        self.cache.lock().values().any(Poll::is_pending)
    }
}
